# Samples — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

## What the deleted sample plans 01, 02, 03 and 12 left behind (2026-09-24)

The plans for breakout (01), asteroids (02), horde (03) and flappy (12) were
deleted on 2026-09-24: every milestone each one named is built. What they still
owe — exit-criterion measurements, one editor test, one debug-tools session and
two design decisions — is in `docs/backlog.md` under _The sample plans — what
they still owe_, one heading per sample. Each sample's gate and what it proves
stay in the ladder table of `docs/plan/sample/00-samples-overview.md`. What
follows is what still binds: the decisions, their reasons and, for horde, the
only copy of its scale measurement.

### flappy (12): why a second game, and what it settled

- **One consumer cannot tell an API from an accident.** Flappy exists to answer
  whether the engine can host a _second_ game without breakout's shape leaking
  into it — the same argument the project paid for with the seam that looked
  complete until `crcbl-wgpu` implemented it and the shader that compiled until
  Dawn read it. Its shape is deliberately unlike breakout's: continuous
  scrolling, procedural spawning, one input, instant loss. The findings it
  existed to produce are `docs/plan/ROADMAP.md`'s _What the second, third and
  fourth games found_; the next sample is judged against this argument.
- **No new engine subsystem.** A game this small appearing to need one is a
  finding to record, not a reason to grow the engine.
- **The ceiling stops the bird rather than killing it**, because a lid that
  kills punishes the safest answer to a low gap.
- **A restart advances the course seed** (`course_seed` in
  `apps/flappy/src/game.rs`) deterministically, so "a new run is a new course"
  and "a recorded script replays exactly" both hold.
- **Gap positions are a pure function of seed and index** (`gap_centre`, an
  integer hash plus affine float operations), never a running generator, so
  client and server agree without sending the pipe list. That the course is the
  same natively and in a browser holds by that construction; no test compares
  the two.
- **Frame-rate independence is asserted at 20, 60 and 240 fps**, the check that
  caught three real bugs in breakout.
- **No network debug module, on purpose.** Flappy runs over `InMemoryTransport`;
  `HostedGame::debug_sections` in `apps/flappy/src/app.rs` contributes the
  course and the audio and nothing else. With breakout it is the check that the
  panel is modular rather than built round a connection.

### breakout (01): the rules its plan set

- **A moving paddle steers the ball; a still paddle mirrors it.** `game::bounce`
  reflects off the contact normal when the paddle stands still; a paddle being
  driven decides the outgoing direction instead, including sending the ball back
  the way it came. That is response policy — the contact still comes from
  `PhysicsSystem::sweep_sphere`. An earlier pass aimed off the contact offset
  across the paddle, which reads as aiming with a bat rather than steering with
  one, and was rejected.
- **No gravity on the ball.** A ball that arcs cannot be aimed; the speed ramp
  per hit is the only thing that changes its speed after a launch.
- **`game::brick_position` is the committed board's generator**, not what the
  game spawns: the board is `apps/breakout/assets/scenes/board.scn/`, and
  `scene::tests::the_committed_board_is_what_the_writer_writes` holds the chunk
  byte for byte to what `Scene::save` writes from it, which keeps the layout in
  one place.
- **The court's wall faces land exactly on the colliders** the ball bounces off;
  near them is not good enough in a game whose subject is where the ball
  rebounds.
- **One debug module, the board.** No network module (in-memory transport — the
  other half of flappy's modularity check) and no audio module, because
  `apps/breakout/src/audio.rs` keeps no counter a row could read. State invented
  to fill the panel would be the panel bending the game.
- **The scope cap**: no power-ups, levels, menus beyond start and game over,
  juice or local multiplayer. Art was never under the cap; reading it that way
  is what put forty bricks through the UI draw list (S1B finding 1).

### asteroids (02): the rules its plan set

- **Rotation is interpolated the short way round.** An angle integrated per tick
  and drawn per frame stutters, and an angle wraps; `lerp_angle` in
  `apps/asteroids/src/game.rs` carries the argument. Asteroids was the first
  sample where a drawn thing turns.
- **What goes in `assets/balance.ron`**: a value the art, the protocol or the
  world's size is baked against stays a constant; a value that only changes how
  the game plays goes in the file. The tick rate and the seed stay flags — a
  value with two doors can be asked for twice and answered differently. The rule
  is written in `apps/asteroids/src/balance.rs`.
- **A malformed balance file is refused by line and column** before the run
  starts, never fallen back on. `--balance <FILE>` is the same loader over a
  `DirSource`, `apps/breakout`'s `--scene` shape.
- **The committed table is the numbers the game already had**, asserted field by
  field by `the_committed_table_is_the_numbers_this_game_shipped_with`, which is
  why the goldens did not move when the constants became data.
- **The scope cap**: no UFOs, hyperspace, power-ups, particles or two-player.

### horde (03): the rules its plan set

- **"Terrain" in the cap means simulation terrain.** Decoration drawn under a
  flat arena is allowed; **pathfinding stays out**, so the props block the
  player and nothing else and the horde seeks in a straight line at a cost that
  does not move with the field. A prop enemies had to route round would be
  pathfinding wearing a tree costume. `game::scatter_props` deals them from the
  _game's_ seed, not the run's, so a restart keeps the cover where the player
  learned it; `game::push_out_of_props` runs once, on the player.
- **One `assets/actors.crpix` at one frame size** (34 texels, the brute's
  collider) holds the player, every enemy kind and both pickups, so the whole
  crowd is one `SpriteRenderer` batch in any emission order and `art::Scene`
  needs no grouping pass.
- **The level-up freezes the field as simulation state**, not a loop pause:
  `freeze_field` zeroes every velocity once, on the tick the screen opens, so a
  seeded script replays the choice and the hot path gains no branch.
- **The potion is a second `PickupKind`, not a second population**, and
  `game::drops_potion` deals it from a `LOOT_HAND` salt on the run seed indexed
  by the kill counter. The rate (one brute in twenty) was settled by
  measurement: at one in three the kiting soak stopped reaching a death.
- **The start screen was argued against and then asked for**; the user's call is
  final, and the reasoning is in `apps/horde/src/game.rs` so nobody
  re-implements the autostart.
- **`--prefill N` is the scale fixture** and starts its own run, since a field
  left at the title screen would time a `run_tick` that returns on its second
  line (`a_prefilled_run_does_not_wait_at_the_title_screen`). It stages `N`
  enemies on a grid fitted to the arena — 0.82 units apart at ten thousand —
  because a 1.25-unit grid collapses 5 280 of ten thousand onto shared positions
  against the walls, which
  `a_prefilled_field_is_the_size_and_shape_it_was_asked_for` catches.
- **The tick's cost tracks local density, not `N`**, so any criterion or budget
  stated as a count has to say which crowd — spread or converged. The
  measurement below is the evidence.
- **The scope cap**: no meta-progression, many weapons or characters, bosses,
  terrain, pathfinding, or particles beyond reused debug-draw primitives — a
  benchmark wearing a game costume.

### horde (03): the scale push, measured

Milestone 3's exit measurement, moved here when the plan was deleted and not
re-measured on the way: like the 2026-09-02 audit (`docs/notes/process.md`), the
move carries these figures on trust. Every figure was taken with the release
binary (`cargo build --release -p horde`) on the reference Linux machine: AMD
Ryzen 9 9950X3D (32 threads) and AMD Radeon RX 7900 XTX on radv (Mesa 26.1.6).

**Conditions common to every table.** There was no display, so every run is
`--headless`, which gives `crcbl-vk` an offscreen image ring at 960 × 720 — the
same acquire → record → submit → present path, not a windowed or vsynced
present. The render and simulation tables were taken **single-threaded**, before
`steer_enemies` went onto `crcbl_jobs::pool`'s `par_for`; the threaded
re-measure is the last table. No browser figure was taken:
`web/run-browser-e2e.sh` runs the demo under SwiftShader, which measures the
software rasteriser.

#### The render side: flat, and nowhere near a budget

Conditions: `--backend vk`, `--wall-clock --fps 0 --tick-hz 1 --frames 20000`.
One hertz of tick keeps the measured frame the render path alone; `--wall-clock`
makes the frame-timing module read the real clock (a headless run otherwise
steps exactly 1/60 s); `--fps 0` keeps the limiter's sleep out of the frame.
**CPU** is the debug overlay's `FrameStats` mean over its 120-frame window;
**GPU** is one resolved frame's `PassTimers` from frame 19 997. Three repeats
agreed to the microsecond except at 5 000, whose CPU read ranged 0.109–0.277 ms
and sprites pass 0.016–0.031 ms; the quiet repeat is reported. The columns are
not addable — GPU work overlaps the next frame's CPU.

|  field | drawn | batches | CPU frame | `arena` GPU | `sprites` GPU | `menu` GPU | `ui-composite` GPU |
| -----: | ----: | ------: | --------: | ----------: | ------------: | ---------: | -----------------: |
|      0 |   305 |       3 |  0.107 ms |    0.005 ms |      0.009 ms |   0.012 ms |           0.004 ms |
|  1 000 |   554 |       4 |  0.096 ms |    0.005 ms |      0.009 ms |          — |           0.005 ms |
|  2 000 |   801 |       4 |  0.109 ms |    0.005 ms |      0.012 ms |   0.009 ms |           0.004 ms |
|  5 000 | 1 555 |       4 |  0.109 ms |    0.004 ms |      0.016 ms |   0.008 ms |           0.003 ms |
| 10 000 | 2 750 |       4 |  0.123 ms |    0.005 ms |      0.027 ms |   0.009 ms |           0.004 ms |

Re-measured 2026-08-07 with `assets/terrain.crpix` and `assets/props.crpix` in
the frame. The `field 0` row is the title screen (the fixture cannot start a
non-prefilled run); from 2 000 up the player dies at about a second and the
death menu is up at the measured frame. `—` is an empty, skipped pass. `drawn`
counts a constant 305 — 300 ground tiles, 4 props and the player — plus the
visible horde: the arena is 96 × 72 units against a view of about 37 × 28, so
what is drawn is bounded by the screen, not the field. `field`, `culled`,
`ground`, `props`, `drawn` and `batches` are horde's `scene` debug-panel
section.

**The frame's shape has moved since.** The UI compositor now draws
`ui-composite` (the HUD) and `ui-overlay` (menu, panel, console), and since
2026-09-15 the menu's frame is drawn in `ui-overlay`, so the `menu` column's
pass no longer exists (`crates/crcbl-render/src/ui_pass.rs`). The table's
`ui-composite` is the whole UI pass as it stood on the day; it has not been
re-measured.

**"CPU frame time flat 1k → 10k on the render side" is met by a wide margin:**
0.096 → 0.123 ms, so nine thousand more enemies cost 27 µs a frame, 0.16 % of a
16.67 ms budget. With `--backend null` the game's own share is 0.008 ms at zero
and 0.033 ms at ten thousand — 2.5 ns per field enemy (the `RenderState` copy,
the cull, the instance build); the other ~0.09 ms is command recording, submit
and present, flat in the field.

**The 1080p UI-pass criterion from the UI plan's exit criteria (under 0.5 ms
GPU)** was measured 2026-08-07 with
`--wall-clock --fps 0 --tick-hz 1 --frames 900 --prefill 10000 --size 1920x1080 --debug-overlay`:
a 0.065–0.068 ms total GPU frame over three repeats, the panel's own pass 0.005
ms (unchanged from 960 × 720; the pass is now named `ui-overlay`), and a
0.150–0.153 ms CPU frame mean. The 2.25× pixel count moved the field sprite pass
0.020 → 0.041 ms. Two orders of magnitude of clearance; the figures come from
the engine's own exit log (`gpu passes` / `frame cpu` lines), so the run needs
no instrumentation.

#### The batching claim: flat in the size of the horde

`batches` is 4 at every populated count (terrain, props, actors, bolt) and 3
when no bolt is in the air. **The claim was never the number** but that it does
not move with the horde; a sheet for a new subject adds a constant, and emitting
one sheet more than once is what breaks it.
`an_interleaved_field_of_every_kind_is_four_batches` and
`ten_thousand_visible_enemies_are_still_four_batches` in `apps/horde/src/art.rs`
catch that, the second packing all ten thousand inside the view in `swap_remove`
order. The count is `crcbl::render::sprite_pass::batch_count`, the pass's own
answer, and `a_batch_is_a_run_of_one_sheet_and_not_a_distinct_sheet_count` pins
`A A B A` = 3.

#### The fill margin: visible, and irrelevant to the budget

The `sprites` pass goes 0.009 → 0.027 ms from an empty field to a full screen of
crowd against a flat 0.005 ms `arena` clear. At 960 × 720 a world unit is 25.71
pixels, so the 34-texel quad is 43.7 pixels square and the 2 445 actor quads at
ten thousand are 4.67 megapixels, 6.8 framebuffers of blended fill;
`SpriteRenderer` has no alpha discard. But
`two_thirds_of_the_shared_frame_is_transparent_margin` measures the average
enemy at **31.5 %** of its quad (62 % grunt at 18 texels, 28 % runner at 13, 10
% brute at 34), so the one-sheet decision costs about **12 µs a frame, 0.07 % of
the budget**. A sheet per kind would buy those 12 µs for a grouping pass and an
emission order to get wrong — not worth it at any count this sample reaches.

#### The simulation side: this is what breaks

Conditions: `--backend null`, fixed-step clock (one tick per frame),
single-threaded. Each figure is a **marginal**: wall time for `--frames 180`
minus `--frames 60` (or 600 minus 480) over the 120 ticks between, best of
three, so start-up and the prefill cancel. Render inside the window is about 0.2
% of the measurement at ten thousand.

| enemies | ms/tick, ticks 60–180 | µs/enemy | ms/tick, ticks 480–600 | µs/enemy |
| ------: | --------------------: | -------: | ---------------------: | -------: |
|     500 |                 0.241 |    0.481 |                  0.939 |    1.877 |
|   1 000 |                 0.418 |    0.418 |                  2.158 |    2.158 |
|   2 000 |                 1.190 |    0.595 |                  6.026 |    3.013 |
|   5 000 |                 4.854 |    0.971 |                 26.289 |    5.258 |
|  10 000 |                14.658 |    1.466 |                 84.087 |    8.409 |

**The two columns are the finding**: the same field one to three seconds in
(spread) and eight to ten seconds in (converged on the player) differ five- to
sixfold, because separation is `N` overlap queries and a query costs the size of
its answer. Single-threaded, spread carried ten thousand at 14.66 ms of 16.67;
converged broke near three thousand, and ten thousand converged was 84 ms — 12
Hz. That superseded 18a's provisional 8–9k, taken during slice 18a on a
1.25-unit grid that at ten thousand is larger than the arena and never
converged:

| enemies (18a, superseded) | ms/tick | µs/enemy |
| ------------------------: | ------: | -------: |
|                       500 |   0.418 |     0.84 |
|                     1 000 |   0.619 |     0.62 |
|                     2 000 |   1.307 |     0.65 |
|                     5 000 |   3.848 |     0.77 |
|                    10 000 |  18.433 |     1.84 |

It read the rise at ten thousand as the working set leaving cache; the cause was
its own grid getting denser against the arena it was clamped into.

#### Re-measured with the pool threaded (2026-08-10)

Same marginal method on a 32-thread machine, varying only `--workers`:

| case                     | enemies | `--workers 1` | `--workers 16` | speed-up |
| ------------------------ | ------: | ------------: | -------------: | -------: |
| spread, ticks 60–180     |  10 000 |      6.508 ms |       3.525 ms |    1.85× |
| converged, ticks 480–600 |   5 000 |     11.508 ms |       2.842 ms |    4.05× |
| converged, ticks 480–600 |  10 000 |     37.383 ms |       7.933 ms |    4.71× |

**Every case is inside the 16.67 ms budget at sixteen workers**, including the
converged ten thousand. It is a different, roughly 2.2× faster machine (37.4 ms
single-threaded against 84.1 ms), so the machine-independent claim is the
speed-up column. The single-threaded analysis had predicted "something like 6
ms" at sixteen cores; it measured 7.9 ms. The two single-threaded wins it named
first are both taken — `PhysicsSystem::body_mut` exists and steering calls
`overlap_sphere_into` with a reused buffer — so P8's headline claim for horde is
met by `par_for` alone, without a parallel ECS schedule.

#### The exit criteria, answered

- **"10 000 enemies at 60 fps render / 60 Hz tick."** Render: yes, with two
  orders of magnitude to spare. Tick: single-threaded, yes spread and no
  converged (5×); threaded at sixteen workers, yes for both. The criterion does
  not say which crowd and needs rewriting (backlog, _What horde still owes_).
- **"CPU frame time flat 1k → 10k on the render side."** Yes.
- **"Playable and mildly fun for 5 minutes."** No. A default run dies at about
  24 seconds with 30 kills and 46 things on the field; at `--prefill 5000` and
  above the player dies in under a second, several enemies inside
  `PLAYER_RADIUS` on frame zero. The target count and the survival target cannot
  both hold in this arena — a design finding, in the backlog.
- **"Profiler capture archived."** The tables are it: `FrameStats` and
  `PassTimers` are the instruments the plan named. No panel image exists; there
  was no display.
- **P7 buys this sample almost nothing.** GPU culling would replace a 28 µs CPU
  cull at ten thousand and the whole render path is 0.12 ms, so P7's ceiling
  here is 0.7 % of the budget. The roadmap put horde behind P7 and P8; it needed
  neither to be built, and P8 was the phase it was waiting on.

## What the deleted sample plans 05, 14, 18, 19 and 20 left behind (2026-09-24)

The plans for viewer (05), quarry (14), sundial (18), alcove (19) and options
(20) were deleted on 2026-09-24 with most of each one built. What they still owe
is in `docs/backlog.md`: viewer's and quarry's under their own headings in _The
sample plans — what they still owe_; sundial's under _What sundial still owes_;
alcove's under _What alcove's bent-direction view did not cover_; options' under
_`apps/options` — what the first slice left_. Sundial's and alcove's ray-traced
rungs are one engine gap with lantern's, under _Ray tracing and the acceleration
structures are unbuilt_. Each sample's gate and what it proves stay in the
ladder table of `docs/plan/sample/00-samples-overview.md`. What follows is what
still binds. **Every figure below was moved from the plans as written and was
not re-measured on 2026-09-24**; each names the test that reproduces it.

### viewer (05): the rules its plan set

- **A tool, not a game — the one sanctioned exception to rule 2.** The viewer
  simulates nothing, so it is client-only by charter; rule 2 exists for games.
  **Rule 11 does not apply**: the point is to show _the user's_ asset unadorned,
  and authored art in the viewport is exactly what it must not do. Rule 4's
  debug panel applies as everywhere.
- **The re-export watch is a poll, not a filesystem-notification dependency.**
  `apps/viewer/src/watch.rs` `stat`s the document four times a second with a
  settle delay, because an exporter writes a `.glb` progressively and every
  platform API reports a re-export as a burst that has to be debounced back into
  one anyway.
- **The glTF → animation conversion is the application's**
  (`apps/viewer/src/anim.rs`), deliberately, because `crcbl-anim` does not
  depend on the glTF importer. The old "no animation playback" cap was withdrawn
  once the engine feature landed; what it protected still holds — the viewer is
  not an animation _tool_: no timeline, no clip selection, no retargeting.
- **Still capped:** material editing, export, scene composition (that is the
  editor), and environment lighting beyond the single directional light and
  exposure.
- **Three doors open a model.** The command line; a window drop, which opens
  through `model::load` over a `DirSource` rooted at the file's own directory so
  a `.gltf` with its buffers beside it works; and a drop on the browser canvas,
  over a `MemorySource`. A page whose dropped file will not parse keeps the
  frame on screen and puts the loader's own sentence on the status bar, because
  a page has no exit code to fail with.
- **The shelf: nine models, one committed.** The `ESC` panel's `SHELF` row lists
  them and Suzanne opens when nothing is asked for, on both hosts. The whole
  shelf is about 138 MB and the repository uses no LFS, so only Suzanne is
  committed; `tools/fetch-shelf.sh` fetches the rest at a pinned upstream commit
  with a sha256 per file (`apps/viewer/assets/shelf.sha256` is the one file
  list, `apps/viewer/src/shelf.rs` the table). **The browser carries three** —
  Suzanne pre-loaded, Avocado and WaterBottle fetched when picked — 18.9 MB
  against a 25 MB budget for the demo's assets; the next-smallest model would
  take it to 28 MB, so the other six are native-only.

**The licence rule for shipped assets (decided 2026-08-30).** The repository is
MIT and its demos are published, so every committed asset is redistributed under
terms a downstream MIT user inherits:

- **CC0 first.** No obligations, nothing a fork can get wrong.
- **CC-BY 4.0 only with attribution** in an `ATTRIBUTION.md` beside the asset
  naming author, source URL and licence, and the same line on the demo's page.
  Not the default, because a fork that drops the file is in breach and nothing
  in the tree would notice.
- **No NC, no SA, no research-only.** A non-commercial clause is incompatible
  with a permissive engine that ships a product; share-alike would relicense the
  demo.
- **Provenance is verified at the source, not remembered.** The verdicts below
  were read on 2026-08-30 from the Khronos `glTF-Sample-Assets` model table and
  each model's own `README.md` at the commit `tools/fetch-shelf.sh` pins, from
  `polyhaven.com/license`, and from the Stanford scanning repository's terms
  page. Re-read before committing a file; an asset's licence is the one on its
  page that day.
- **Decided 2026-08-30, the user: the model demos use the CC0 models from
  Khronos' `glTF-Sample-Assets` and nothing else.** Not Poly Haven models, not
  CC-BY sets with an attribution file. Poly Haven stays named only as the CC0
  source for a PBR _texture_ or an HDRI if a rung ever needs one the Khronos
  shelf lacks.

| Model                                                                                     | Source                          | Licence                                                                 | Verdict                                                                                                                                                                                                               |
| ----------------------------------------------------------------------------------------- | ------------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Suzanne** (the monkey)                                                                  | Khronos glTF-Sample-Assets      | CC0-1.0 (UX3D 2017)                                                     | **In.** The one-mesh material fixture; ~8k triangles subdivided, a normal-mapped variant is ours                                                                                                                      |
| Stanford bunny (the rabbit)                                                               | Stanford 3D Scanning Repository | research-only, no commercial use without permission                     | **Out.** "Not to be used for commercial purposes, nor appear in a product for sale" — a redistributed MIT demo is exactly that. Any rabbit here is a different rabbit                                                 |
| Avocado, BoomBox, Corset, Lantern, WaterBottle, BarramundiFish, FlightHelmet, SciFiHelmet | Khronos glTF-Sample-Assets      | CC0-1.0                                                                 | **In**, as the gallery's shelf: full metallic-roughness sets with normal, occlusion and emissive maps, sized for a browser tier                                                                                       |
| AntiqueCamera                                                                             | Khronos glTF-Sample-Assets      | CC0-1.0 plus `LicenseRef-LegalMark-UX3D` on a logo baked into a texture | **Out.** The mark's own text says UX3D "reserves the right to remove the Mark or unilaterally change the terms of use" — the obligation-to-track the rule above exists to avoid. Read 2026-08-30 at the pinned commit |
| DamagedHelmet                                                                             | Khronos glTF-Sample-Assets      | CC-BY 4.0 / CC-BY-NC 4.0 dual                                           | **Out.** The dual licence is a trap for a fork; SciFiHelmet is the same kind of model under CC0                                                                                                                       |
| MetalRoughSpheres                                                                         | Khronos glTF-Sample-Assets      | CC-BY 4.0                                                               | Allowed by the licence rule with attribution — the BRDF ladder's calibration chart — but outside the user's CC0-only decision above                                                                                   |
| Poly Haven models and textures                                                            | polyhaven.com                   | CC0                                                                     | Textures and HDRIs only, per the decision above                                                                                                                                                                       |
| Duck, BrainStem, CesiumMan                                                                | Khronos glTF-Sample-Assets      | SCEA / Poser EULA / CC-BY                                               | Out, or not worth their terms                                                                                                                                                                                         |

There is no rabbit on the Khronos CC0 shelf, and the Stanford scan is the one
the user meant and the one that cannot ship — so the demos have no rabbit.
Recorded so nobody re-derives it.

### quarry (14): the rules its plan set

- **The three geometry paths draw the same scene, not the same pixels.** The
  paths differ in selection granularity — per cluster on `MeshShader`, per
  instance on `IndirectCount` and `IndirectPerBatch` — so the claim is the same
  scene at the same quality budget. Where lantern proves the two lighting paths
  agree, quarry proves the three geometry paths do, and it is the only place the
  QEM generator's output is _looked at_ rather than measured: an error metric
  can be within budget while a seam is visibly wrong.
- **What the QEM generator can be shown to hold today is border locking and
  determinism, and nothing more.** `crcbl_scene::simplify`'s header is the
  account: position borders are locked, not optionally; UV and normal seams,
  material boundaries and skin weights are not constrained. The plan's "Proves"
  once read as though all four were proven; they are a requirement until topic
  25's attribute slice lands (backlog, _Three of the four QEM properties quarry
  claims to prove are not implemented_).
- **One instance of one mesh, on purpose** — so all of the reduction is cluster
  culling, and a scene where both culls matter is horde's job (see _DECIDED —
  quarry keeps one face_ below).
- **The LOD tint and the heatmap are mesh-path only**, because a per-cluster
  number exists only where selection is per cluster.
- **The browser page runs `IndirectPerBatch` with per-instance LOD and opens on
  the animated dolly**, because a page showing one held frame proves nothing
  about a cut that follows the camera.
- **Hard cap:** no gameplay, streaming, HLOD (topic 25 schedules those and this
  sample must not smuggle them in), impostors, a second scene, or an authoring
  tool for meshlet parameters.
- **Exempt from rules 2, 10 and 11** — one face, one instance, a camera and a
  debug view selector; nothing simulates, so no `World`, no system, no
  `GameModule`, and no `.crpix` art because the subject is geometry density.
  **Rules 4 and 12 apply in full**: three paths is the widest selector in the
  engine, so path reporting matters most here.

### quarry (14): measured (2026-08-20)

Taken 2026-08-20 on an AMD Radeon RX 7900 XTX (RADV NAVI31, Mesa 26.1.7-arch1.1)
by `apps/quarry/tests/device/`, which is where each number can be reproduced.
Where an exit criterion asked for a **human** to look, this says so instead of
standing in for one.

**The face.** 8192 triangles at level 0, one instance of one mesh. The uniform
cut at a 16 px budget draws level 1, 4096 triangles.

**Where the reduction comes from — `all_of_the_reduction_is_cluster_culling`.**
Over seven reported frames down the fixed dolly the camera's instance cull kept
**1 of 1 every time**, and the amplification stage kept 26 to 34 clusters. All
of the reduction is cluster culling, and the reason is the scene rather than the
renderer: quarry places one instance, so the instance cull has one thing to
decide about.

**Which test does the rejecting —
`the_three_cluster_counts_add_up_to_the_cut_they_were_taken_over`.** Standing at
the dolly's far end at a 256 px budget, the descent chose a cut of **58**
clusters (`[15, 31, 12, …]` finest level first), and the amplification stage
answered: **30 kept, 28 rejected by the frustum, 0 by the normal cone.** The
three partition the cut, which is what that test asserts.

**The cone rejects nothing on this face, and that is the correct answer.**
Measured separately by pinning the eye underneath the surface so every cluster
faces away from its viewer: clusters kept moved from 44 to 42, and covered
pixels not at all. A rough surface gives clusters cones wider than a hemisphere,
which `crcbl_shaders::meshlet::ClusterBounds::cone_cutoff` records as a cutoff
at or below zero and `cluster_survives` skips outright. The value of the split
is that the panel can _say_ the cone did nothing; before it could only say 30 of
58 survived, which is equally consistent with the cone doing all of the work.

**The three paths against each other**, from the committed goldens rather than
from a device, so any reader can reproduce it:

| dolly stop            | mesh-shader against either indirect path     |
| --------------------- | -------------------------------------------- |
| start (standing back) | 233 px differ (0.47%), max channel delta 118 |
| end (inside the face) | 0 px differ                                  |

The two indirect paths are identical to each other at both stops, as expected:
they run the same per-instance selection through different draw machinery. The
difference is at the **far** stop — standing back, screen-space error varies
most across a face that recedes 180 m, so per-cluster selection has the most to
disagree with per-instance selection about; inside the quarry everything is at
the finest level and all three draw the same triangles.

**What no measurement can close:** whether those 233 pixels read as "the same
scene at the same budget", and the seam review against `quarry_face(CELLS)` and
`quarry_tile`, are judgements. Neither has been made (backlog, _Quarry's two
human judgements and its browser budget are untaken_).

### sundial (18): the rules its plan set

The rendering records sundial produced after it was built are in
`docs/notes/rendering.md` — _What sundial still owes_ (its surprises),
_`apps/sundial` took the atmosphere_ and _The shadow filter selector leaves
three things owed_. These are the fixture's own rules.

- **The sample exists for the artefacts a still frame hides.** Acne,
  peter-panning, cascade seams, swimming edges and a penumbra of one width at
  every distance are each invisible in a screenshot or until the light moves, so
  the plaza gives each a surface to appear on — a ground plane at a grazing sun,
  a plinth resting _on_ it, a colonnade crossing the cascade split, casters at
  graded heights — and the sun moves on a scripted, pausable, scrubbable clock.
  A demo where an artefact cannot appear proves nothing about the bias.
- **The ladder is `pcss`, `disc` and `box`**, selected by `r_shadow_filter`. The
  rotated disc took the place of the Poisson set the plan first named (topic
  45's ninth decision, in `docs/notes/rendering.md`); virtual shadow maps are
  refused there with a reason and this sample does not reopen them.
- **The seam is per fragment, out of `FrameUniforms::shadow_filter`**, because a
  scene pass cannot be recorded twice under a scissor; `crcbl_render::split`
  counts the column.
- **The atlas viewer is a full-screen pass in `crcbl-render`, drawn in display
  space after the tonemap**, not a branch in `mesh.slang`: the atlas is one
  image the whole frame shares rather than a function of any fragment, and its
  greys must not move with the exposure.
- **A diagnostic golden is read, not only compared.** `plaza-cascades` stands
  beside two readings — inside cascade 0 clear of the band, and past the split
  placed from `crcbl::render::Cascades`' own split — with the same two places
  overlay-off as the control; `plaza-atlas` asserts the amber border round the
  near cascade's cell and the black letterbox. A golden alone cannot say which
  tint or which grey it is looking at.
- **The page's controls are HTML, and each export answers with what the engine
  holds after the write**, so the page keeps no copy. The clock is game state,
  not a console cell, so its two controls go through `crate::sun`'s channel and
  are adopted on the next fixed step.
- **The first split depends on the near plane**, which is why
  `apps/sundial/src/plaza.rs`'s `NEAR` is half a metre (recorded in
  `docs/notes/rendering.md`, _What sundial still owes_).
- **Exempt from rules 2, 10 and 11** — no game state, no `World`, no
  `GameModule`, and the subject is shadows rather than pictures.

### sundial (18): the readings behind milestones 2 to 4

All from `apps/sundial/tests/golden.rs`, on radv and lavapipe, as the plan
recorded them on 2026-09-05 and 2026-09-06.

**The bias pair —
`the_two_bias_counts_trade_acne_against_the_plinths_own_contact`.** The counts
are console variables, `crcbl_render::shadow::r_shadow_bias` and
`r_shadow_normal_offset`, floats in texels of the cascade a fragment landed in,
each declaring the constant it replaced as its default; `Cascades::params` reads
the cells rather than the constants. Five arms of one frame at
`sun::GRAZING_TICK` — what ships, each count at zero, each count pushed — and
two readings off each: what share of `ACNE_CENTRE`'s block of open pavement is a
self-shadowing dot, and the **shadow term** at `plaza::PLINTH_CONTACT` and at
five stations further along the plinth's shadow. Three claims:

- **Zero either count and the pavement roughens; the contact does not move.**
  The normal offset at zero takes the block to `41.53%` dots on radv and
  `41.53%` on lavapipe, the constant bias at zero to `3.26%` and `3.23%`,
  against `0.00%` on both as the sample ships — and the contact's term is
  `70.73` on radv and `70.44` on lavapipe on all three arms, to a hundredth.
- **Push the constant bias and the shadow comes off the plinth.** At 96 texels
  the contact's term falls to `6.37` while the pavement past it still carries
  `67.01` — peter-panning, a lit gap between a caster and its shadow, rather
  than a shadow that has gone. Under 88 texels the contact keeps its shadow
  outright and past 104 the shadow has left the whole visible strip.
  Eighty-eight is large because the depth pass keeps front faces, so a bias has
  to cross the block's whole 1.2 m depth; a thin caster loses its contact at a
  small count.
- **Push the normal offset twenty times as far and the contact does not move.**
  At 40 texels its term is the shipped one to a hundredth, on both adapters,
  though the shadow's far end has begun to go — the seventh decision's claim
  that a sideways move keeps a contact, measured. At 44 the contact and the
  pavement beyond it go together.

Widened to four rows over three rungs, each read against its own control. The
`disc` rung reads what the shipped rung reads to a hundredth on both adapters,
except at the pushed bias, where its contact falls to `0.45` on radv and `0.37`
on lavapipe against `6.37` and `6.36`; its peter-panning window runs from 92 to
100 texels, so 96 sits inside it. The `box` rung reads every claim but one:
under the narrowest kernel the shipped normal offset covers the acne block on
its own, so zeroing the constant bias leaves `0.0000%` dots — that clause is
read on a fourth row at 1.5 texels of normal offset, where `29.1737%` (radv) /
`29.5090%` (lavapipe) trades against `4.6467%` / `4.9581%` at the shipped bias.
`HELD_OFFSET` is read on `box` at a station of its own. Measured and **not**
read: the top of the arc, `sun::NOON_TICK`, where zeroing the offset draws
`0.0000%` dots and the bias sweep takes the contact and the pavement past it
away together — `176.00`/`173.33` at 50 texels, `91.65`/`96.31` at 52,
`0.00`/`0.00` at 56 — leaving no count with a gap to read; and
`plaza::counter_camera`, which frames the acne block but has
`plaza::PLINTH_CONTACT` behind its eye. `plaza::pavement_camera` — a metre back,
half a metre off axis and 25 cm higher, looking across the colonnade — is the
second pose that frames contact, stations and block at once inside cascade 0,
and every constant reads on the far side of its bound from there too. The
sabotage: `Cascades::params` handing the shader the constants again instead of
the two cells makes every setup's four moved arms draw its own shipped frame
byte for byte.

**The cross-fade —
`the_colonnades_shadow_crosses_the_cascade_split_without_a_step`.** It measures
the **shadow term** (the frame with shadow passes off, less the frame with them
on, so the pavement's Lambert falloff cancels), walks every column of the
colonnade's shadow at offsets either side of its edge, and bins each walk into
shells of **distance from the eye** — the quantity `sun_visibility` selects a
cascade by. Each walk's step between the shells either side of the split is held
to the steepest step the same walk shows clear of the band. With the band:
`2.24` against `1.43` on radv, `2.33` against `1.16` on lavapipe; with
`CASCADE_FADE_FRACTION` at zero and every artifact regenerated, `17.49` against
`1.24` and `17.55` against `1.41` — the sabotage. The `disc` rung: `2.98`
against `1.43` with the band, `39.24` against `4.02` collapsed. The grazing sun:
`0.63` against `1.30` with the band, `3.32` against `0.09` without. Measured and
**not** read: `box`, whose walk clear of the band is too flat (`0.12`/255 radv,
`0.04` lavapipe) for the ratio to separate the two; and `counter_camera`, whose
frame holds no sample of any walk in the shell window. The walk reads only
pavement the arm's camera can see and no lamp reaches, which
`plaza::hidden_from` and `plaza::lamplit` answer off the plaza's own geometry.

**The penumbra —
`the_penumbra_widens_with_its_casters_height_under_pcss_and_not_under_disc`.**
Three cubes of one size at graded heights over one plane, widths walked in
**metres of pavement**. Under `pcss`: 0.0400 / 0.0560 / 0.1000 m on radv and
0.0400 / 0.0560 / 0.1040 on lavapipe, a ratio of 2.500 and 2.600; under `disc`
0.0400 / 0.0440 / 0.0400 on both, a ratio of 1.000 — the half that says the
widening came from the blocker search rather than the scene. Re-read on
2026-09-06 with the atmosphere: only the tallest `pcss` counter moved, and only
on lavapipe, by one step of the walk.

**The side by side —
`the_seam_runs_the_console_filter_on_the_left_and_the_shipped_one_on_the_right`.**
It walks every rung the engine declares, because a rung wired to its neighbour's
branch is the failure one pair cannot see: every column but the split's is exact
on both adapters for each, with `disc` standing 9.234 and 228.562/255 from
`pcss` down the two halves and `box` 27.387 and 258.306. Read with the
antialiasing resolve out of the arm since 2026-09-06: the resolve walks an edge
for up to `crcbl_shaders::cmaa2::MAX_LINE_LENGTH` texels and would carry the
seam past its band (`SEAM_BLEED`'s doc carries the sweep).

### alcove (19): the rules its plan set

- **AO darkens the ambient term and nothing else, and the court is built to show
  it.** The sun's azimuth, the fixed camera's eye ray and the slot's axis are
  one line, so the floor at the bottom of the slot is in full sun at any depth
  and the crease claim is about a directly lit surface.
  `occlusion_scales_the_ambient_term_and_leaves_direct_light_alone` measures it
  as a difference of differences with the sun switched off, not as a ratio.
- **Flat, near-untextured surfaces by choice**, since texture detail is exactly
  what hides an AO artefact — which is also why rule 11 does not apply.
- **Every control drives an `r_ssao_*` variable by name through
  `crcbl::render::console_table()`** — the seam a typed console line goes
  through — so a pause-panel row, a page control and a typed line cannot
  disagree, and the page keeps no second copy of the state.
- **The bent-direction view is one `DebugView` cell**, written by `N`, the
  `BENT VIEW` row, the page button, `--bent-view` and a typed `debug_view`, so
  the last writer draws and the panel reports what is in force. A term that
  steers where ambient is sampled from cannot be reviewed as a grey image, which
  is why the view exists.
- **The silhouette rim has its own golden from a second pose**
  (`court::rim_camera`), because at the fixed camera the sphere is a few dozen
  pixels across and a one-pixel halo is invisible to a person or a block
  average.
- **A cost for a technique the frame did not draw is not reported.**
  `OcclusionCost` reads the frame's `ssao` and `ssao-shipped` timing rows, so
  there is a per-technique cost only while the seam is up.
- **HBAO is refused in topic 18, and there is no specular occlusion** until
  topic 18 decides it is a term of its own; a scalar AO is the wrong quantity
  for it and this sample must not imply otherwise.
- **The page draws `LightingPath::Rasterised` by construction** — WebGPU exposes
  no ray query — so it compares two screen-space gathers. Its controls are HTML
  because the seam is walked with `,` and `.` natively, which a phone lacks.
- **Exempt from rules 2 and 10**, on the viewer's ground.

### options (20): the rules its plan set

- **The only application that writes a player's setting.** Before it, the one
  writer in the workspace was `crates/crcbl-cli/src/settings_cmd.rs`, so the
  layer that stores a player's choices had been exercised by everything except a
  player. The round trip it proves is its own restart.
- **`[engine.video]` may only clamp downward, and an absent key clamps nothing**
  (the clamp rule in `docs/notes/backends.md`). A settings screen is the first
  thing that can violate it, so **requested and resolved are shown separately**:
  the _time_ half is `menu::NEXT_START_MARK` on a row that applies at the next
  start; the _clamp_ half is `menu::HELD_MARK` — `frame_limit = 240` in a binary
  launched at 60 reads `240 fps, held to 60 fps`, from `LoopConfig::limit`.
- **Antialiasing is a replacement inside the resolve slot, not a clamp** — a
  player picking CMAA2 where the camera asked for FXAA is asking for a different
  filter (the antialiasing ladder's eighth decision, `docs/notes/rendering.md`).
  The row is born on the rung `RenderEffects::DEFAULT_STACK` carries.
- **Rows are derived from the engine's tables, never spelled twice.** One switch
  per `crcbl::settings::VIDEO_KEYS` entry, so a new key gets a row with no
  second spelling.
- **No row for a setting that cannot be applied live and observed.** Display
  mode, resolution and present mode wait for a window seam that applies them and
  reports what the window system did; a control that does nothing is worse than
  one that says so, and a key with no reader is labelled.
- **A preset label is derived each frame, not stored.** Picking a tier writes
  every key it owns; touching any one drops the label to custom. The tier row is
  the first row, above every key it writes.
- **Ladders step by rung.** A hand-written value between rungs goes to the rung
  above on a step forward and the rung below on a step back. `RESET` writes what
  an absent key means — `DEFAULT_ANISOTROPY` for anisotropy, every effect
  allowed for the switches — rather than the bottom of a ladder.
- **Three buses carry content** — a tone on `Bus::Music`, a noise tick on
  `Bus::Sfx`, a click on `Bus::Ui` — and `Screen::set` is the one place a gain
  changes, moving `Mixer::set_bus_gain` in the same call that writes the key.
  `Bus::Voice` and `Bus::Ambience` have no content and their rows say
  `(silent)`.
- **A browser with no store must be told to the player, never swallowed.** OPFS
  is the only browser backend (`crates/crcbl-store/src/lib.rs` records the
  IndexedDB fallback as still to come); a settings screen that silently forgets
  is the worst version of this bug.
- **Non-goals:** input rebinding (its own screen, owed in `docs/backlog.md`
  under _Input: no rebind screen, no input inspector, no `crcbl input` CLI_),
  accessibility settings beyond the catalogue, a migration format beyond topic
  14's, and per-monitor or per-adapter profiles, which topic 15 refuses.
- **Exempt from rules 2, 10 and 11** — the settings are the content.

## What the deleted sample plans 13, 16 and 24 left behind (2026-09-25)

The plans for lantern (13), bracket (16) and tumble (24) were deleted on
2026-09-25 with about half or more of each one built. What they still owe is in
`docs/backlog.md`: lantern's and bracket's under their own headings in _The
sample plans — what they still owe_, lantern's ray-traced half under _Ray
tracing and the acceleration structures are unbuilt_; tumble's under _What the
deleted 24-tumble plan left unbuilt_, with the solver rungs its last two rooms
wait on under _Contact solver rung 6_ and _Buoyancy and wind force providers_.
Each sample's gate and what it proves stay in the ladder table of
`docs/plan/sample/00-samples-overview.md`. What follows is what still binds.
**Every figure below was moved from the plans as written and was not re-measured
on 2026-09-25**; each names the test or command that reproduces it.

### lantern (13): the rules its plan set

- **The sample makes graceful degradation checkable.** Ray tracing is Vulkan and
  D3D12 only; macOS, iOS and every browser render the rasterised twin. A raster
  path nobody looks at carefully ships broken to most of the audience, so
  lantern exists to make "the fallback also works" something a human has seen
  rather than something a plan asserts. The Pages demo runs `Rasterised` by
  construction, and that is the point: the browser is the raster path's largest
  audience.
- **Both lighting paths draw the same scene, not the same pixels.** A scene that
  reads correctly on one path and wrongly on the other is a defect in whichever
  is wrong. **One material model** feeds both — the same material table and BRDF
  — which is what keeps two lighting paths from becoming two renderers.
- **A lesser path is held to the same golden as the best.**
  `the_room_draws_the_same_on_a_path_below_the_devices_own`
  (`apps/lantern/tests/golden.rs`) draws the room on the adapter's own selectors
  and again with every feature above `ForcedPaths`'s floor withheld, and holds
  both arms to one reference: a lesser path is a constraint on data layout
  rather than a separate renderer, so a difference is a bug in the better path
  and a per-path reference is what would bless it. The subtraction goes through
  `crcbl::engine::ForcedPaths::optional_features`, the function `--force-*`
  uses, so it cannot drift from the flags. The arms are asserted to differ
  exactly when the adapter offers a withheld flag, so a device already at the
  floor is a checked claim rather than a silent skip.
- **Every effect toggles independently, from each of the three layers of the
  resolution order** — the camera stack (`room::MONITOR_STACK`, the in-scene
  monitor's view, which drops the reflections), `[engine.video]` (`gpu.rs`'s
  `video_effects`) and the programmatic override (`--no-shadows`, `--no-ao`,
  `--no-reflections` and the pause menu's rows). A toggle that works from only
  one layer is a finding about the resolution point. A menu row is
  read-modify-write on the **programmatic** layer and nothing else
  (`crcbl_lantern::toggled_effect`), so it never discards a decision another
  layer made and can turn an effect back on after `--no-*`; what a row shows is
  the **resolved** answer, and an effect the device cannot draw reads
  `UNAVAILABLE` rather than `OFF`, so the panel never offers a tick that does
  nothing.
- **A golden claim is a ratio between two blocks of pixels**, a block the effect
  works on against a control block it does not touch, so a frame that merely got
  brighter fails; each is re-run at twenty-five times the pixel count so it is a
  claim about the room rather than about the sampling.
- **Neither metal surface has an ambient term, and that is the model.** Ambient
  scales the diffuse albedo and a conductor has none; reflection is the metals'
  only fill (argued in `docs/notes/rendering.md`, _What the deleted 44-lighting
  plan left behind_). The mirror panel's foot is a real screen-space hit; the
  rest of that face, and the rough block above `ROUGHNESS_CUTOFF`, take the
  irradiance-probe environment. The probe grid is a blurry low-frequency field
  and the only answer the raster path has for anything outside the frame; ray
  tracing is what replaces it, and the panel's `unbuilt` section says so rather
  than faking it.
- **The probes are the engine's, filled every frame.** `crcbl_lantern::bounce`
  only places them from the room's own dimensions and ships their rows zeroed
  with `ProbeUpdate::EveryFrame`; `crcbl_render`'s reflective-shadow-map updater
  fills them from the sun's near cascade and the lamp's shadow faces, under the
  no-bake rule (`docs/notes/rendering.md`). One bounce, no history; the fixed
  camera deliberately puts a floor in full sun beside a wall in shadow, the
  configuration a second bounce would change most.
- **The acceleration structures, when they are built**, are built at bake (BLAS)
  and refit per frame (TLAS) from the same instance data the cull pass reads,
  with build cost and refit cost shown separately on the panel.
- **Hard cap:** no gameplay, no second scene, no authoring tools (the workbench
  pattern is sparks' and hud's), no physically-measured validation against a
  reference renderer, no denoiser research, and no effect beyond what topic 18
  ships.
- **Exempt from rules 2 and 10** on the viewer's ground — there is no game
  state, so no `World`, no system, no `GameModule`; **from rule 8** (decided
  2026-09-06) because the rule is about positional game events and a fixture has
  none to cue, with hud as the precedent; and **from rule 11** because the
  subject is 3D lighting. **Rules 4 and 12 apply in full.**

### lantern (13): measured (2026-08-14)

On radv, by `apps/lantern/tests/golden.rs`:

- **`every_effect_toggles_and_the_frame_says_so`**, four states at 1280×960: the
  shadowed floor goes 51.0 → 141.3 with the atlas off while the sunlit floor
  does not move; the plinth's contact corner goes 51.5 → 58.9 with occlusion off
  while open floor does not move; the mirror panel's foot goes 29.8 → 1.3 with
  the march off while the part of the same face that reflects nothing goes 20.1
  → 0.0 — that part is the probe environment, which the reflection pass also
  supplies.
- **`zero_probes_only_remove_the_ssr_and_rough_fallbacks`**, at the golden's own
  256×192: the panel's reflecting-nothing point reads 20.3 with authored probes
  and 0.0 with the rows zeroed, its foot 51.6 and 49.0, and the brass block's
  camera-facing face 97.4 and 89.7 — so the panel's upper face is probe data
  outright, its foot a real screen hit, and the block mostly the sun's own
  specular with the environment on top.
- **The path pair:** radv resolves the two arms of
  `the_room_draws_the_same_on_a_path_below_the_devices_own` to
  `MeshShader / Bindless` and `IndirectPerBatch / ArrayPages`, and llvmpipe
  reports mesh shading and bindless too, so the lavapipe leg draws the same
  pair.

### bracket (16): the rules its plan set

- **Why it is its own sample.** breach and shard are LAN only, which strands
  topic 27's tier 3, the `crcbl-mint` chain and every matchmaking concern with
  no consumer. Matchmaking quality is a property of a **population over time**,
  so attaching it to a real game would need a real playerbase; with the match
  resolved by a stub, a synthetic population of any size runs in CI,
  deterministically, in seconds. What it proves that nothing else does is
  **reliable-channel, request/response traffic** — no snapshots, no
  interpolation, no tick.
- **No hosted service.** A hosted matchmaker was considered and declined: it
  would have been the project's only infrastructure, and the browser client it
  enabled was the sole justification for WebTransport and the WebSocket
  fallback, which left topic 23 as a result (the LAN rule in
  `docs/notes/simulation.md`). The server is a process on the same machine or
  LAN, found the way breach and shard find theirs.
- **The match stub is deliberately not fair**: a seeded roll weighted by the
  participants' true skill. An outcome that always favoured the higher rating
  would make convergence trivial and prove nothing.
- **Glicko-2, transcribed step by step from Glickman's paper and checked against
  its worked example** (`rating::tests::the_paper_s_worked_example`). A rating
  system nobody can falsify is a number generator. Why Glicko-2 rather than Elo
  is the ladder's _scale_: the record is `docs/notes/simulation.md`, _Glicko-2
  lands in bracket, and the lever is the step size_.
- **Pairing adjacent players on a rating-sorted queue is optimal for the line,
  not an approximation.** The total gap over pairs drawn from a line is
  minimised by pairing adjacent points, so no search finds a materially better
  set, and it is `O(n log n)` for the sort — which is what lets thousands run in
  CI.
- **`Rating` has no constructor taking arbitrary points** outside the crate's
  own tests: one starts provisional and moves only through `settle`, so a
  non-finite rating has nowhere to enter from. An `f64` parameter would have let
  a NaN in to spread through every later match.
- **`bracket sim` is the sample's subcommand, not the engine CLI's** —
  `bracket sim [--seed N] [--players N] [--ticks N]`, routed in
  `apps/bracket/src/main.rs` before the ordinary parser, so a word that parser
  does not know is genuinely unknown. The run length is in **ticks**, because a
  tick is what the matchmaker advances; the match count is an output.
- **The demo takes no input and runs `Sim` directly.** Every decision comes from
  a hash over the seed and a counter, so a run is reproducible from its seed.
  Routing it through a tick-shaped loopback as things stand would look like the
  transport claim while not being it; queueing, leaving and reporting are
  commands, and the command path is engine work (backlog). The rating update
  runs the host's `exp`, `ln` and `powf`, so two platforms agree to the
  precision anything reports rather than byte for byte; nothing compares
  bracket's output across platforms.
- **Rule 2 is owed, not exempted**: `apps/bracket` opens no `World` and
  implements no `GameModule` today, and its exit criteria assume it will.
  **Exempt from rule 11** — the subject is a service and its client is a UI.
  Rule 4 applies, and its network module is the interesting one: the netgraph
  reporting service traffic rather than tick traffic, which a panel built for
  tick traffic may read as broken — a finding about the panel if so.
- **Hard cap:** no game, cosmetics, progression beyond rating, chat or social,
  elimination tournaments (the name is about pairing, not a tree), anti-cheat,
  region or latency routing, or a production identity service. A feature that
  needs a real playerbase to evaluate does not belong here.

### bracket (16): measured (2026-09-10)

Runs of `bracket sim` at 2000 ticks and seed 1. `rating error` is the mean
distance from true skill at the end of the run; a larger population starts
closer, because its mean is nearer the middle of the skill range.

| players | matches | wait, ticks | pairing, points apart | rating error, points |
| ------- | ------- | ----------- | --------------------- | -------------------- |
| 2       | 6       | 30.83       | 400.6                 | 194.9                |
| 4       | 150     | 31.01       | 261.2                 | 29.0                 |
| 8       | 859     | 9.00        | 107.6                 | 42.5                 |
| 16      | 2971    | 3.48        | 59.3                  | 27.1                 |
| 64      | 15099   | 1.42        | 37.5                  | 34.5                 |
| 256     | 63146   | 1.06        | 18.0                  | 36.2                 |
| 1024    | 255607  | 1.01        | 5.8                   | 39.7                 |

**Convergence at a stated size:** 64 players over roughly fifteen thousand
matches land between 25.7 and 34.5 points of mean error, from 254.0 — seeds 1 to
5 gave 34.5, 27.0, 25.7, 31.7 and 28.1 over 15 099 to 15 241 matches. The
tolerance bracket claims is **under 40 points on a 1000-point skill range**, the
figure to hold a change against.

**The degenerate row is both halves of the trade-off at once.** Two players give
the matchmaker one pair: it waits thirty ticks _and_ pairs four hundred points
apart, and six matches happen in the run. Four players halve the gap with the
same wait; the wait falls only once there is a queue to pick from, and from
sixteen up it is a tick or two. Pairing quality is monotone from four players up
while rating error is flat within its seed spread — what the ladder needs is
matches, not crowds.

### tumble (24): the rules its plan set

- **Tumble shows where the physics engine is weak, and by how much** — the
  user's framing: it "will show us where the gaps are in the physics engine and
  its performance". A scene the engine cannot produce ships **labelled as the
  gap it is**, on its room's hint line, never hidden or faked, and the rung that
  closes it turns the label off. The scene list is the contact solver's
  acceptance suite (its rung table is in `docs/notes/simulation.md`) and the
  ball pit is its benchmark.
- **Rule 9's "no game-code collision math" is kept**: no bounce is drawn that
  the engine does not compute. **Exempt from rule 11**, on lantern's ground.
- **Determinism is a pinned constant.** Every room starts the same way every run
  and the room key never reaches the simulation, so the hash at
  `scene::CHECK_TICK` is `scene::PINNED_HASH` natively and the browser gate
  holds the wasm build to the same constant.
- **The Tower room's pyramid runs alone in its own system** at the default
  contact settings, so the room's solver time is the pyramid's and comparable
  with Box3D's benchmark; the column and the dominoes share a second system at
  the same defaults, the column's cubes asking for twelve substeps through
  `PhysicsSystem::set_substeps`, because at 30 Hz twenty cubes buckle.
- **Bullets fire a charge held for one tick**, so the tick's speculative
  contacts know nothing of the shot's speed. The stated speed limit is the one
  tested: 80 m/s at point blank.
- **The bridge breaks one weak link at 15 kN.** An 800 kg anvil put 20 kN on
  every hinge — a chain carries its tension end to end — so a threshold on every
  hinge snapped all twenty-two in two ticks.
- **Settle has no room of its own**: its claim is that every room with contacts
  comes to rest, with awake and sleeping bodies and islands on the panel. "A
  dropped ball wakes exactly the island it lands on" is a `crcbl-phys` test,
  `crates/crcbl-phys/tests/settling.rs`, not a scene; the mesh's own proving
  scene is `crates/crcbl-phys/tests/meshes.rs`.
- **Debug reading is lazy**: the debug module constructs its reading only inside
  the visible panel's `debug_section`, so a hidden panel costs no extra
  canonical physics hash. `debug_reading_hashes_only_while_visible` covers
  hidden, visible and re-shown panels, and restoring the eager reading made its
  hidden-panel assertion fail.
- **Non-goals (hard cap):** soft bodies, cloth, fracture and fluids — a topic
  each; vehicles; gameplay; reduced-coordinate articulations, which the contact
  solver declines.

### tumble (24): measured (2026-09-23)

- **Settle.** The pit asleep at tick 1000, the Tower room's pyramid at 58 and
  its column and dominoes at 276 until the next flick wakes them, and the wall
  326 ticks after its spawner is stopped (it never is on the page). After rung 4
  gave one-point contacts twist friction — before it, a ball rested spinning
  about the vertical for ever — the wall sleeps 367 ticks after its spawner
  stops and the pit at tick 966.
- **Bullets**, over twenty seconds: with rung 4's sweeps none of the forty shots
  or forty spins tunnelled, 364 bodies were stopped and 2.92 s of their motion
  dropped; with the sweeps off every one tunnelled
  (`nothing_tunnels_through_the_rooms_sensors`).
- **Bridge.** The cradle carries 1.981 kg·m/s in and 1.973 out. Before the anvil
  the bridge's hinges held to 3.8 mm with crates on it, a crate putting at most
  2.5 kN on a hinge; the weak link goes at tick 758; the ragdolls' joints held
  to 5.3 mm and 69 mrad down the stairs.
- **The lazy debug reading**, sequential release Vulkan runs at 960×720 on the
  Radeon ICD's discrete adapter, the same paused state at 256 ticks and 320 pit
  balls, 500 frames timed after warm-up: hidden-panel p50/p95 went from
  0.907/0.928 to 0.597/0.616 ms, and 0.876/0.897 to 0.589/0.630 ms in the repeat
  pair — p50 reductions of 34.2% and 32.8%. Visible-panel p50/p95 was
  1.083/1.118 ms eager and 1.101/1.123 ms lazy, no visible-panel improvement.
  The timer is CPU frame time — page, menu and instance preparation,
  acquisition, submission and frame-ring waits, excluding start-up and stepping
  — not isolated GPU time. Hashes and visible hash rows matched throughout.

## shard's doused zone was never lifted, and the numbers if it should be (2026-09-04)

**Considered and declined, so it is not re-proposed.** When the no-bake rule
removed `apps/shard`'s baked irradiance volume, its doused zone stopped being a
picture: one quantised colour covers 95% of the canvas where the browser gate's
blank-frame control asked for under 85%. The control was re-derived instead —
see `web/tools/browser-e2e.mjs`'s `TORCH_INSET` for what it now measures and why
a share of the whole canvas cannot answer the question any more.

Lifting `zone::house_light`'s ambient was the other candidate and it works. The
sweep, over the browser gate on radv, every value inside the `< 0.05` that
`the_house_light_is_an_ambient_floor_and_not_a_sun` pins:

```text
  ambient (r, g, b)         lit mean   doused mean   doused flat
  0.012, 0.011, 0.014  ×1     16.01         6.68        94.9%
  0.024, 0.022, 0.028  ×2     17.25         8.23        94.0%
  0.036, 0.033, 0.042  ×3     18.42         9.62        50.1%
  0.048, 0.044, 0.049  ×4     19.37        11.20        58.4%
```

The share holds at 94% and then collapses, which is a quantisation cliff rather
than a trend — below it the room's surfaces all round to one 8-bit value. So ×3
would have restored the original control with margin.

**It was not taken because `house_light`'s own doc argues against it**: "a flat
term bright enough to see by would be a room that looks lit whether or not
anything lit it". At ×3 the doused room reads 0.52 of the lit one where it reads
0.42 today, which is a visible change to how a doused zone looks. The table is
kept here so that a later decision to make the zone legible when its torches are
out does not have to re-run the sweep.

**What is still true and is nobody's bug**: the zone's only surviving light is
the shrine spot, which is faint and stands in one corner, so the doused frame
carries almost nothing that varies. `light::torches` records the measurement
that says so.

## Considered and declined — do not re-propose

Each was checked against the rule that DRY is about duplicated _knowledge_, not
duplicated shape.

- **The `DebugModule` impls.** Same shape, genuinely different numbers, and each
  sample's doc argues against sharing. This is the seam working.
- **Action sets and key bindings.** A game is entitled to rebind alone.
- **`horde/src/controls.rs`.** Its shared parts are _already_ engine —
  `TouchStick`, `PauseControl`, `CONTROL_STYLE`. What is left is horde's.
- **`HudStrings` and `draw_hud`.** The keys and strings are content.
- **`fn still`** in three `art.rs` files — it returns a _game-local_ struct, so
  sharing it would put a per-sample type in the engine.
- **`with_shell` and `open_the_window`.** `open_the_window`'s title, app id and
  error type are the game's, and a wrapper taking all three needs six positional
  arguments with two adjacent `&str`s among them. `with_shell` looks like the
  others and is not: `apps/horde` builds its clock from `!options.real_clock()`
  rather than from `headless`, and `apps/lantern` opens its window through a
  different signature. Extracting it would need a callback per difference.

### towers' audio non-goal was withdrawn (2026-08-27)

**Correction, not a gap.** The non-goals list said "audio (engine gap)".
`crates/crcbl-audio` ships — device seam with a real-time streaming thread
natively and an `AudioWorklet` in the browser, plus mixer, spatial and synth
modules — and breakout, flappy, asteroids and horde all emit spatial cues
through it. Sample rule 8 applies to towers with no exemption. The doc has been
corrected; no work is owed until the sample exists.

### sparks takes a rule 2 exemption that was unwritten (2026-08-27)

**Recorded, not owed.** `docs/backlog.md` already flags that `apps/bracket` and
`apps/sparks` "carry none and claim no exemption" from sample rules 2 and 10.
For sparks the exemption has now been written into
`docs/plan/sample/10-sparks.md` on topic 20's own grounds — visual-only VFX are
"client + GPU ... zero gameplay reads, zero readbacks", and "gameplay-relevant
particles are not particles — they're entities". For bracket the answer is the
opposite (see below), so that backlog entry can be closed for sparks and
narrowed to bracket.

## Records from individual samples

Each record below is about one sample and names it.

### What `apps/shard`'s fight slice left out, and why (2026-08-26)

`apps/shard/src/foe.rs` is three archetypes, one ability each, a sighting ray
and a cooldown, and nothing else. What was considered and left out:

- **Anything resembling navigation.** An engaged foe walks _straight at_ the
  character and slides along whatever it meets. `docs/plan/24-navigation.md`
  names `arena` as its forcing function, so shard does not force it either — the
  same position `apps/breach` takes. The visible cost is on the two posts in the
  far hall: a foe engaged from there has the shrine doorway between it and the
  character, and it will slide along a doorpost rather than walk round it. The
  husk's post is _in_ that doorway partly for this reason.
- **A facing on any body.** Every body in this sample is a
  `crcbl::greybox::capsule`, which has no front to turn — the same admission
  `zone::Figure` already made about the character. `apps/breach`'s `BotView`
  carries a `facing` and shard's `foe::FoeView` deliberately does not.
- **Sound.** Rule 8 asks for spatial audio and the fight is the cue grammar that
  would want it most — a warden's wind-up is a sound before it is a colour. The
  sample plays nothing at all, here as in slice 1.
- **Aim error, blocking, dodging, stagger, or any resource but health.** Each is
  a system, and milestone 1's cap is "a handful of enemy archetypes and
  abilities".
- **A weapon.** The character's cleave is a constant reach and a constant damage
  in `foe`, not an item — `docs/plan/34-inventory.md`'s kit is the open decision
  recorded below, and a weapon would be its first consumer.

### The fight slice pins two things the browser gate depends on (2026-08-26)

Both are asserted natively so a later change fails a test rather than a gate:

- **Every post in `foe::POSTS` is out of `foe::NOTICE_M` of the spawn and out of
  the frame the zone opens on**, with three heartbeats of walking as margin.
  `no_foe_can_reach_the_character_where_the_zone_opens` and
  `no_foe_is_in_the_frame_the_zone_opens_on` in `apps/shard/src/foe.rs` are what
  hold it. The second projects each post's capsule centre through the camera the
  frame is actually drawn from and asserts it is outside the frustum
  _vertically_, which is the bound that does not move with the canvas's aspect.
- **Why it matters, measured.** The browser gate's lighting block asks for a
  canvas that does not change _at all_ while the torches are out. Sabotaging
  `Foe::advance` to engage unconditionally was run: the doused window came back
  "4 distinct frame(s) in 4 sample(s) … swinging 0.00", so the mean luminance
  barely moved and the **frame hash** did — a body walking through shot is
  enough to redden a check that has nothing to do with the fight. The fight
  block therefore runs _after_ the lighting block, and the posts are where they
  are.

### `apps/shard`'s zone has no roof, and that is deliberate (2026-08-26)

Not a missing piece — do not "fix" it. `zone::WALL_TOP_Y` is the height of the
walls, and nothing is drawn above it. Measured: with ceiling slabs over the open
tiles, `camera::Iso` put the eye five metres above the character at the
isometric elevation, so every frame the browser gate sampled was the _top_ of
those slabs — 93% black, and byte-identical from one frame to the next. The
module docs in `apps/shard/src/zone.rs` carry the argument and the measurement.

The same geometry decided where the character starts: the eye sits about two and
a half tiles behind them, so a spawn near the outer wall looks out over the top
of it. `zone::LAYOUT`'s `S` is at the mouth of the corridor for that reason, and
moving it back towards the entrance will bring the dark foreground back.

### `apps/shard`'s camera cannot be pitched, zoomed, or pointed (2026-08-26)

`camera::Iso` holds a fixed elevation and a fixed distance and offers four
bearings. That is the rig the plan asks for, and it means the browser gate never
exercises a _look_ input on this page — `apps/breach`'s gate is the only one
that does. Considered and declined for slice 1: a pitch control would be a
second camera behaviour to test and would let a visitor put the eye back above
the walls, which is the failure the entry above describes.

### `apps/breach`'s practice bots are dumber than the plan's, on purpose

`apps/breach/src/bots.rs` is patrol, notice, shoot, lose interest, and nothing
else. What was considered and left out, each with the reason:

- **Anything resembling navigation.** No path query, no poly mesh, no steering,
  no avoidance. `docs/plan/24-navigation.md` names `arena`'s bots as its forcing
  function, not breach's, and forcing a navigation pillar out of a practice map
  would be building the subsystem from the wrong demo. A bot walks
  `map::practice::ROUTES` and slides along whatever it bumps into, which is
  `CharacterController::move_and_slide` doing it rather than the bot.
- **Cover use, flanking, squads, difficulty tuning.** Each is a behaviour tree
  or a utility system, and the sample has no place to put one yet. Milestone 2's
  5v5 bots are where that question is actually asked.
- **Aim error.** A bot that can see the player hits them, every round. There is
  no spread, no reaction time and no first-shot delay beyond the cadence, so the
  only thing between a player and a hit is cover. That makes the demo legible
  and the browser gate's control exact — `fired` above `taken` is cover and
  nothing else — and it is also why standing still in the open is punished
  harder than a practice map should punish it. Ballistics (topic 28) is where
  spread belongs.

### `apps/breach` keeps its own copy of the yaw→direction step

`apps/breach/src/camera.rs::walk_direction` and the orbit-measured conversion
are the same three lines of trigonometry with opposite signs, because the two
demos measure yaw in the two conventions their cameras came with — breach's is
`Flyer`'s and puppet's was `OrbitCamera`'s. **This pair should not be merged
into one function**: the whole claim it exists to make is that the conversion
belongs to the rig rather than to `crcbl-phys`, and a helper taking a flag for
which convention it was handed would put the choice back in a third place.
Recorded here so the idea is not re-proposed every time somebody greps for
`walk_direction`.

The orbit half **has** since moved, and by the route this note named: the
trigger it stated was "a third sample on the orbit basis", `apps/shard` was it,
and `OrbitCamera::walk_direction` in `crates/crcbl-render/src/orbit.rs` is where
the conversion for that convention now lives — beside the rig whose measure it
is, not in the physics crate. Puppet and shard call it and keep their own tests
against the `Camera` their own yaw builds. Breach's copy stays where it is until
a second first-person demo arrives, at which point the same move is available
beside `Flyer`.

### The viewer frames the document's geometry, not the geometry it draws

`apps/viewer`'s `model::world_bounds` unions one `Aabb` per glTF primitive,
pushed through its instance's composed transform. Those are the primitives the
**document** declares, and `build_render_scene` may skip some of them — so a
document with a skipped primitive is framed a little wide.

It errs in the safe direction (wider, never tighter) and every skip is printed,
so this is a refinement rather than a defect. Fixing it needs `RenderScene` to
say which `(mesh, primitive)` each of its `instances` came from, or to carry
per-instance bounds; neither exists, and adding one to `crcbl-scene` for a
cosmetic framing difference was not worth it here.

### SHIPPED — one golden-harness fixture, and the curve moved to `crcbl-golden`

Record of the decision and of what landed. Option (a) was taken on 2026-09-06
and built the same day.

**What is in the tree now.** `apps/crcbl-sample-test` is a lib-only workspace
member — no `src/main.rs`, so `tools/check-windowed-samples.sh` and every demo
list pass over it — taken as a `[dev-dependencies]` entry by `apps/asteroids`,
`apps/breakout`, `apps/flappy`, `apps/horde` and `apps/hud`. It carries
`required_backend`, `adapter_line`, `SampleRun` (the run-the-binary half that
was `screenshot_from_a_real_run`) and `Block` (`brightness` and `channel` over a
fixed half-extent, which was `brightness`/`channel`/`channel_mean`). Each suite
keeps its own constants, its own `inspect` and its own goldens; nothing about a
picture moved.

`srgb_encode` went the other way, into `crcbl_golden::srgb` as `encode` and
`encode_level`, re-exported at that crate's root as `srgb_encode` and
`srgb_encode_level`. **Not** into the sample-test crate, and the reason is the
dependency direction: the fixture needs `crcbl` — for
`crcbl::backend::BACKEND_ENV_VAR` and to spawn a sample binary — while four of
the curve's callers are `crates/crcbl`'s own e2e binaries, which would then have
to dev-depend on a crate that depends on them. The curve depends on nothing at
all, so it belongs on the leaf every golden suite already reaches. Its callers
now: `crcbl`'s `render_e2e`, `hal_seam_e2e`, `forward_e2e::depth_probe`,
`forward_e2e::shadow` and `sprite_e2e`, plus `apps/sundial` and `apps/alcove`.
`crcbl_golden::srgb`'s own unit tests pin it against IEC 61966-2-1's anchors —
the two ends, the linear segment's slope, the knee, and two rows of the 8-bit
table — rather than against a run of the code.

**The copies had drifted, and this is what differed.** Diffed before the move:

- `adapter_line` was byte-identical in all five.
- `required_backend` differed only in the harness script it names, which is now
  an argument.
- `brightness`, `channel` and `channel_mean` differed in **shape**:
  `apps/breakout` threaded the block's half-extent through as a parameter that
  every call site passed `BLOCK` to, while the other four closed over the
  `BLOCK` const directly. `Block` binds it once, which is the four's behaviour
  with the one's explicitness.
- `screenshot_from_a_real_run` differed in **what it checks**, and this is the
  drift that mattered: `apps/breakout` and `apps/flappy` assert the summary's
  _simulated_ tick count moved — the check flappy's suite first got wrong by
  asserting the loop's count instead, which passed a frozen build twice — and
  `apps/asteroids`, `apps/horde` and `apps/hud` never gained it. The fixture
  keeps each caller's behaviour (`simulation_advanced`) rather than quietly
  adding an assertion to three suites; that gap was closed the same day, below.
- The `srgb_encode` copies differed in two ways. `apps/sundial`'s and
  `apps/alcove`'s return a level out of 255 where the other four return `[0, 1]`
  — both shapes are kept, as `encode_level` and `encode`. And `sprite_e2e`'s
  used `1.055f32.mul_add(…, -0.055)` where every other copy used
  `1.055 * … - 0.055`; a fused multiply-add rounds once instead of twice, so
  that copy was a different function by a fraction of an ulp. It now uses the
  same one as everything else.

The options weighed, kept because (b) will look attractive again:

- **(a) A small support crate under `apps/`** that the sample test targets
  depend on. Clean, and it is where the knowledge belongs; costs a workspace
  member that exists only for tests.
- **(b) `#[path]`-include one file** from each suite. No new crate and no
  manifest churn, but the included file is compiled once per suite and each gets
  its own copy of every `const` — fine here, surprising later.
- **(c) Leave it.** Five copies of about a hundred lines, and the next sample
  makes six.

**Both loose ends the move left were closed on 2026-09-06.**

**Every sample suite now checks that its simulation advanced.** `apps/asteroids`
and `apps/horde` carry a `sim_ticks` field on their `Summary`, read from a
`Game::ticks_run` that is now public as breakout's and flappy's already were;
`apps/hud` reads the accessor it already had. All three front ends print it as
`(N simulated)` beside the loop's own count — which is where
`SampleRun::simulation_advanced` reads it — and all five suites set the flag.

The backlog's open question was whether the three summaries carried that figure.
**None of them did**, so the field alone would have failed the run rather than
checked it, and its guess that `apps/hud` is "a ticker rather than a game loop"
and would not want the check was wrong in the direction that matters. Measured
by freezing each `Game::tick` behind an early return:

- `apps/asteroids` and `apps/hud` red on the new assertion and on nothing else.
  Frozen, hud exits 0, presents every frame it was asked for, and prints
  `hud: 60 frames, 59 ticks (0 simulated) … (wave 1, 30 page commands, …)`: the
  wave, the page's command count and the loop's own tick count all read exactly
  as they do on a live run. The simulated figure is the only thing that notices.
- `apps/horde` is the one sample where an older assertion fires first.
  `--prefill` _queues_ the start edge for the next tick rather than poking the
  state, so a horde whose tick does nothing never reaches `Playing` and fails
  `stdout_contains` before the count is looked at. Its red was measured instead
  by dropping the `ticks_run += 1` line alone, which leaves the run otherwise
  intact: `horde: 60 frames, 59 ticks (0 simulated) … Playing`.

**`srgb_decode` followed `srgb_encode`** into `crcbl_golden::srgb` as `decode`,
re-exported at that crate's root. Its callers were `crcbl`'s `render_e2e` and
`sprite_e2e`, `apps/alcove`'s `eotf`, `apps/viewer`'s `linear_of` and
`apps/breakout`'s `to_linear` — the last two inside `#[cfg(test)]` modules under
`src/`, which is why `apps/viewer` now takes `crcbl-golden` as a dev-dependency
(`apps/breakout` already had one). The callers that hand it a byte keep a
one-line adapter of their own; nothing else was reshaped.

**The five copies differed in exactly one thing: where they change arms.**
`render_e2e` switched at `0.040_449_935` and the other four at the
specification's rounded `0.040_45`. **Nothing measured moved, and nothing
could**: the two thresholds are about 6e-8 apart, no `byte / 255` lands between
them, and for the signals that do the two arms answer within 3e-9 of each other
in linear light. `KNEE_ENCODED` is `12.92 × KNEE_LINEAR` now so that there is
one knee rather than two spellings of it — tidiness, and its doc comment says so
rather than claiming a fix. What the new sweep of `decode(encode(x))` holds is
the pair being inverses at all, which is the claim that would have caught a
transcription slip in either direction. `crcbl_render::mip`'s
`srgb_to_linear`/`linear_to_srgb` stay where they are, for the reason given
above.

### DECIDED — quarry keeps one face, and documents the degenerate split

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

quarry's plan asked in its exit criteria for the reduction to be attributed:
"how much of the reduction is instance culling and how much is cluster culling,
because a single total hides which one is working". quarry now records both, and
the answer is **all of it is cluster culling** — the instance cull keeps 1 of 1
on every frame, because the scene is one instance of one mesh.

That is a true answer and a degenerate one. The criterion exists because a real
scene has many instances and the two culls can mask each other; with one
instance, "the instance cull did nothing" and "the instance cull is broken"
produce the same frame, and the test can only assert the count is 1.

**Making it interesting is a change to what the sample depicts**, which is why
it is a question rather than a task. Placing four or nine faces in a row, some
outside the frustum, would give the instance cull something to reject and make
the split a real measurement — at the cost of a scene the plan describes as "one
dense scene", and of pools four to nine times larger. The alternative is to keep
one face and say plainly in the sample's own docs that this criterion is
answered but not exercised.

Not a blocker either way: the numbers are recorded and asserted as they stand.

### DECIDED — quarry commits six goldens, two dolly stops per path

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

`apps/quarry`'s exit criteria ask for "golden frames per `GeometryPath` from the
fixed dolly". Everything needed to produce them exists — the dolly, the three
forced paths, the offscreen readback — and the question is what to commit, which
is a scope call rather than a technical one.

**The problem is that the three paths do not draw the same pixels**, by design.
Measured: at a one-pixel budget all three cover an identical 28,650 of 49,152
pixels, but at sixteen the mesh path draws levels 1 and 2 per cluster while the
other two select one level per instance, and they land four pixels apart. So a
single golden cannot serve all three, and three goldens per dolly stop is 9 × 3
images for one sample.

The options:

1. **One golden per path at one dolly stop** — three images. Cheapest, and it
   catches a path that breaks outright. It does not catch a path that breaks
   partway down the dolly, which is where LOD lives.
2. **One golden per path at the ends of the dolly** — six images. Covers the
   coarse and fine extremes, which is where the cut differs most.
3. **No goldens; keep the measured assertions.** What quarry asserts today —
   coverage, the per-cluster cut, the uniform cut's walk, the triangle counts —
   is stronger than an image comparison for everything except \_what the face

   is stronger than an image comparison for everything except _what the face
   looks like_, and weaker for exactly that. The engine already has 37 goldens
   under `crates/crcbl/tests/golden/`, none of which is quarry's content.

**The measured argument for (2):** the numbers quarry records are all counts,
and every one of them would be unchanged by a shading bug — a face lit from the
wrong side covers the same pixels, walks the same rungs and draws the same
triangles. That is precisely the gap a golden closes and no counter can.

Also unresolved either way: goldens are compared on a runner with a software
rasteriser, and quarry's frames here come off an RX 7900 XTX. The engine's own
goldens are shared across backends with a tolerance, so the mechanism exists;
whether this content passes it on lavapipe is unmeasured.

**`MeshShading` being `Unwritten` on dx12 does not block the gate**, which is
the thing worth writing down. The paths are not one-per-backend: they are
reached by **subtracting features from a single capable adapter**.
`crates/crcbl/tests/render_e2e.rs` does exactly that, and it passes here on an
RX 7900 XTX — eleven `..._draws_the_same_frame_on_every_geometry_path` tests,
with the harness printing

```text
asked for MESH_SHADER: true,  adapter has it: true, drew through MeshShader
asked for MESH_SHADER: false, adapter has it: true, drew through IndirectCount
spot_shadow on MeshShader against IndirectCount — 0 channel(s) differ, budget 0
```

So Vulkan reaches `MeshShader` and `IndirectCount` on one device and compares
them pixel-for-pixel. The **third** path, `IndirectPerBatch`, is not in that
cross-backend suite — it is covered by `crcbl-vk`'s own `vk_e2e/draw_gen.rs`,
whose three arms name all three paths and which opens its device without
`DRAW_INDIRECT_COUNT` on purpose, because no adapter that suite can see would
ever select the floor path. A three-way quarry gate would follow `draw_gen.rs`'s
shape, not `render_e2e.rs`'s.

**Which sample came next was decided** — quarry, over `breakout-as-wasm` (P6A, a
`wasmtime` `WasmHost` seam), lantern's second half (S4B, blocked on P7C's ray
tracing) and orbit (S5, behind three physics phases). quarry was the only one
whose prerequisites were already built.

## The two sample `gpu.rs` files that stopped being identical

`apps/breakout/src/gpu.rs` and `apps/flappy/src/gpu.rs` were once identical once
the game's name was normalised away. They are not any more — `cmp` says so — and
flappy's camera scrolls where breakout's is fixed.

**Kept because the decline was right and that is worth not re-arguing.** The
shared shape looked like a plausible `crcbl-render` bundle: orthographic camera,
sprite pass, menu pass (since folded into the UI pass), UI pass over
`GpuContext`. The stated reason for leaving it alone was that two 2D games at
the same stage resembling each other is not the same as one piece of knowledge
written twice, and that the failure mode would be a helper with two callers
needing a flag per caller. That is exactly what a scrolling camera would have
become. The trigger is unchanged: revisit when a third game wants the bundle.

## Considered and declined

Record; the one open finding this list leaves — whether `crcbl_ui::hud` gains a
`Label` colour or is deleted — is in docs/backlog.md under the same heading.

- **Adopting `crcbl_ui::hud`'s `Hud`/`HudPanel` in the four samples.** It was on
  the audit's list as "the engine feature was already bought", and it is not:
  the type does not do what any of the four HUDs needs.

  **`Label` has no colour.** Colour lives on `Style`, one per panel, so a
  panel's labels are all one colour. Every sample draws its stat line yellow,
  its state line pale blue and — breakout — its lives line green, which is three
  colours in one panel and is not expressible. That alone ends it.

  Two smaller mismatches behind it. `HudPanel` sizes itself from its content,
  where horde's backdrop width is a **measured** constant with a test putting a
  stated worst-case run through the real `FontAtlas` and requiring it to fit;
  auto-sizing throws that guard away. And `Hud::render` routes button clicks,
  which a read-only stat panel has no use for.

  **What is actually shared between the four is not the drawing.** Each has a
  private `HudStrings` that rebuilds its strings only when the numbers behind
  them change — the caching avoids the `format!` work each frame. **It does not
  stop the frame allocating, though four doc comments say it does** (corrected
  2026-08-15): `DrawList::text` takes `impl Into<String>` and stores
  `text.into()` into a `DrawCommand::Text { text: String }`, and every sample
  calls it with `hud.score.as_str()` — so a fresh `String` is allocated per text
  command per frame and dropped by `DrawList::clear`.
  `apps/breakout/src/app.rs`'s `draw_hud` names "the sandbox's 'a steady-state
  frame allocates nothing' property" as the reason the cache exists, and that
  property is not delivered by this mechanism. **This does not change the
  decline below** — the `Label`-has-no-colour argument ends adoption on its own
  — but it removes the strongest stated reason the samples' version is worth
  keeping, and it means the real fix would be on `DrawList` (a borrowed command,
  or an arena) rather than in any sample. But the structs differ in their fields
  and their cache keys, because each game shows different numbers: that is
  duplicated _shape_, not duplicated knowledge, and the logic under it is three
  lines. Extracting it would be an abstraction over a coincidence.

- **Building the demos' export names in `web/engine/demo.js` from the sample's
  slug.**
  `exports[\`**crcbl\_${sample}\_frame\`]`would delete the thirty-line`bind`block from each`web/demos/<name>/main.js`and is the obvious way to write it. Declined because it defeats the gate:`web/tools/check-exports.mjs`learns which exports the JS depends on by scanning for a literal`.**crcbl\_…`and fails when one is missing from the artifact. Verified both directions — with the names spelled out, renaming`\_\_crcbl_breakout_frame`to`…\_framee`in`main.js`fails the check with that symbol named; behind a template literal the scan sees nothing and a typo becomes a`TypeError`
  in somebody's browser. The per-sample file is the price of keeping the check
  able to fail.
- **Folding the demo pages' "what is actually running" prose into a partial
  too.** Its opening paragraph differs between breakout and flappy by two words
  ("high score" / "best score") and its second paragraph differs materially —
  flappy's explains the seeded course, breakout's names swept-sphere collision.
  Templating it would mean the layout carrying three prose variables, which is a
  generator, not a partial. The shared blocks are the ones that are identical
  and structural: the window, the loop's keys, and the console note.
- **Reformatting `web/tools/browser-e2e.mjs` with prettier.** It is not
  prettier-clean at the width the rest of `web/` uses — confirmed against the
  version at `HEAD`, so it predates this work — and this slice touched only a
  three-line comment in it. Reformatting the whole gate file to fix a whitespace
  complaint would bury that comment in a diff nobody can review. Worth doing on
  its own, with the gate run either side of it.
- **Fixing the multi-sheet sprite bug in the shader, by adding
  `SV_StartInstanceLocation` back on.** It works, and it is one line:
  `sprites[instance + base]` with `uint base : SV_StartInstanceLocation`
  restores the `BaseInstance` that `SV_InstanceID` subtracts, giving the
  absolute index that the old `draw(0..6, batch.instances)` needed. Measured
  with slangc 2026.14: the SPIR-V comes out with the `OpIAdd` next to the
  `OpISub` and no extra capability beyond the `DrawParameters` the file already
  declares.

  Declined for two reasons. First, `slangc` **rejects that semantic for WGSL** —
  `error[E55202]: system value semantic 'sv_startinstancelocation' is not supported for the current target`
  — so the source would have to be `#if`-split per target, and there is no
  target macro to split on (probed: `__TARGET_SPIRV__`, `SLANG_SPIRV`,
  `__SPIRV__`, `__TARGET_WGSL__` are all undefined; only `__SLANG_COMPILER__`
  is), so the split would have to ride on the `-D` per target that
  `crates/crcbl-shaders/tools/compile-shaders.sh` and `build.rs` now pass —
  `CRCBL_TARGET_SPIRV`, `CRCBL_TARGET_WGSL`, `CRCBL_TARGET_MSL`,
  `CRCBL_TARGET_HLSL`. Second and worse, the WGSL half would then be correct
  **because Slang's two lowerings disagree**: `SV_InstanceID` becomes
  `InstanceIndex - BaseInstance` on SPIR-V and a bare `@builtin(instance_index)`
  on WGSL, and only the SPIR-V one matches HLSL. A Slang release that made WGSL
  consistent with the rest would silently break the browser, with nothing in
  this repository pointing at the cause. Always drawing from instance 0 depends
  on neither lowering.

- **A dynamic offset on the instance _storage_ buffer rather than a per-batch
  constant block.** The obvious shape — bind `sprites` with `dynamic: true` and
  offset it to the batch — needs the binding's declared **size** to be fixed at
  bind-group creation while `offset + size` must stay inside the buffer, so the
  size would have to be "the largest batch", which is a per-frame quantity the
  group is not rebuilt for. Batches would also have to be padded to
  `min_storage_buffer_offset_alignment` (256 on WebGPU) rather than packed at
  `INSTANCE_STRIDE`. The constants block is 80 bytes and fixed, so the same
  mechanism costs nothing there.

- **Sharing `apps/*/src/audio.rs` and the best-score file between the two
  samples directly.** The duplication is real (findings 4 and 5) and the fix is
  in the engine, not in a crate the samples share between themselves: a
  `flappy-and-breakout-utils` would be a third place for the same code to rot,
  and it would hide the evidence that `crcbl-audio` and `crcbl-store` are
  missing a layer. **Vindicated**: both layers were built where the evidence
  said they belonged — `crcbl_audio::synth` and `crcbl::store::record::Record` —
  and the samples adopted them.
- **A `visible` check inside `DebugPanel::layout`.** It was written, and it
  could not be made to fail: `add` refuses to gather while hidden and
  `set_visible` drops what was gathered, so a hidden panel has no sections and
  the emptiness check already returns `None`. A guard that no test can reach is
  a guard that reports "passed" for reasons unrelated to what it guards, so it
  was deleted and the reasoning left in its place.
- **A `DebugSection::row` taking `String`s.** It takes `fmt::Arguments` instead,
  so a module writes `row("fps", format_args!("{fps:.1}"))` and formats straight
  into a `String` the section already owns. The ugly signature buys a
  steady-state section rebuild that allocates nothing, which matters for the one
  widget whose job is not to disturb the thing it is measuring.
- **Tinting one brick sprite four ways instead of authoring four frames.** It is
  the cheaper sheet and it is what `app.rs`'s colour table used to do. Four
  frames is what lets the rows differ in their _shading_ — a lit top edge and a
  shaded bottom in each row's own hue — which a single tinted rectangle cannot
  express, and it is what a sprite sheet is for. The cost is 96 × 8 texels
  instead of 24 × 8.
- **Re-randomising flappy's course from a clock.** A restart advances the seed
  deterministically (`course_seed(seed, runs)`) instead. A clock would make the
  course unreproducible, and the sample's exit criterion is that a recorded
  script replays to the same score.
- **Authoring flappy's background bands at one texel per sprite unit.** They are
  drawn at `art::BACKGROUND_SCALE` = 2 instead. At `TEXELS_PER_UNIT` = 20 a hill
  wide enough to read as a hill is a couple of hundred texels of hand-written
  rows for a silhouette with two bumps in it; the pipe is deliberately **not**
  scaled, because its caps are measured in texels and scaling would stretch
  them. If the bands ever gain detail that the doubling makes obvious, redraw
  them rather than adding a second scale knob.

## The debug-overlay retrofit: what was rejected, and one engine doc that is now stale

Record; the finding that is not fixed is in docs/backlog.md under the same
heading. Breakout, flappy and asteroids now contribute `DebugModule` sections
(`BoardStats`, `CourseStats`, `FieldStats`, plus `DebugModule for Audio` on
flappy and asteroids), wired through `HostedGame::debug_sections` the way
horde's `SceneStats` already was. What was considered and left out:

- **Breakout has no audio section.** `breakout::audio::Audio` keeps no counter
  at all — no `plays`, no `dropped` — so a row would have meant adding state to
  the game for the panel's benefit. Rejected on those grounds. If breakout ever
  grows a `plays` vector the way flappy's did, the section is three lines.
- **Ball speed was the only invisible breakout number.** `GameLogic::ball_speed`
  is the difficulty ramp and nothing displayed it; everything else breakout
  knows (score, lives, state, high score) is already in `HudStrings`. A
  `paddle`/`ball x,y` row was considered and dropped as a number the player can
  see.
- **Asteroids does not repeat the wave.** `HudStrings::refresh` already draws
  `Wave: {wave + 1}`, and two numbers on screen under the same word that differ
  by one is worse than one.
- **No entity count for breakout.** It would have meant a new
  `Game::entity_count` accessor, and breakout does not churn: it spawns the grid
  once and despawns bricks until a restart respawns them. Flappy and asteroids
  both already had the accessor because both are churn samples.
- **Four audio modules, four different facts — deliberately not shared.** Horde
  reports `dropped` (it is the only sample with a `MAX_VOICES` cap), flappy two
  cue counts plus live voices, asteroids three cue counts plus whether the held
  engine loop is sounding, breakout nothing. The `label: value` shape is common;
  the knowledge is not, and the samples are separate binaries, so extracting one
  would mean a new crate or a change to `crcbl-ui`.

## `apps/lantern` is at milestone 1a: what it owes next (2026-08-14)

Record; what the sample owes, and the one coverage gap the room produced, are in
docs/backlog.md under the same heading.

### Findings the first real room produced

- **The sun's shadow peter-panned at contacts, and it is closed.** A lit strip
  along the foot of every wall and a sawtoothed band at the head of the back
  wall; two bias slices took the strip 0.60 m → 0.26 m and topic 45's seventh
  decision (`docs/notes/rendering.md`) — the normal offset, 2026-08-28 — took
  the rest. What it left is "The normal offset scallops one silhouette's foot"
  above, and "What the sun's shadow bias still leaves open" below.
- **A single-quad wall casts no shadow at all.** Back faces are culled in the
  shadow pass as well as the colour one, so an inward-facing quad is invisible
  to the sun. lantern's first frame was an evenly lit floor with a window that
  did nothing; the room is built of slabs for that reason and `room::SHELL`
  records it. Worth knowing before the next scene is authored: it is not a bug,
  it is what `CullMode::Back` means on an open surface, and nothing warns about
  it.
- **A gap in a shell leaks light and reads as an artefact.** Stopping lantern's
  ceiling at the room's own footprint left a slot over the top of every wall;
  the sun came through the one above the window wall and laid a band along the
  back wall that looked exactly like a shadow-map failure. The ceiling caps the
  walls now. Same class as the row above: authoring hazard, not an engine
  defect.
- **`crcbl::screenshot::Scene` is not where lantern belongs, and that is
  decided.** Considered: adding a `Scene::Lantern` variant so
  `crcbl screenshot --scene lantern` would work. **Declined** — the room is an
  _application's_ scene description and putting it in `crates/crcbl` would make
  the engine own sample content, which is the exact thing this sample exists to
  prove is no longer necessary; and the enum's stated job is one variant per
  engine shader pair that has pixels of its own, which lantern adds none of.
  What it needed instead was a way in: `OffscreenSetup::open_forward` takes a
  caller-built `ForwardScene` and reuses the surface, adapter pin, ring,
  readback barriers and row unpadding. That is rule 1 working as designed — a
  sample needing a backdoor is an engine API gap, filed and fixed in the engine.
- **`crcbl new` scaffolds a shape lantern would have had to undo.** The template
  is one `src/main.rs` with a bin target and a `Game` with a simulation in it.
  lantern needs a lib target — an integration test cannot reach a bin crate's
  room — and has no simulation. Not a defect in the template, which is aimed at
  games; recorded so the next fixture does not start from it either.
- **`OffscreenSetup` leaked a swapchain and a surface when a scene refused.**
  `Scene::Dunes`' "no amplification stage" arm destroyed its own renderer and
  returned, leaving both behind. Fixed in the same change as `open_forward`,
  because the new entry point made the refusal path reachable from an
  application.

## Asteroids' balance table: two records (2026-09-07)

### The per-size rock table stayed in code

**Considered and declined.** `RockSize::radius`, `speed`, `score` and `spin` in
`apps/asteroids/src/game.rs` are four `const fn`s over one enum. `speed` and
`score` are pure balance and would belong in
`apps/asteroids/assets/balance.ron`; `radius` is the size the baked `.crpix`
sprite is drawn to and `spin` is bounded by that sprite's texel count, so those
two cannot move. Splitting one table across a file and the code was judged worse
than leaving it whole. Moving it would mean a per-size sub-struct in the RON
file and three more lookups on the split path.

### `--balance` refuses a file name that is not an asset key

**Behaviour that surprised us, not a bug.** `Balance::read_file` reads through
`crcbl::assets::DirSource`, whose keys allow only ASCII alphanumerics, `.`, `_`
and `-`. So `--balance "my tuning.ron"` is refused with "path escapes the
storage root", which is the right refusal wearing the wrong words. Reading the
file with `std::fs` instead — the shape `apps/lantern/src/args.rs`'s
`read_stack` has — would drop the constraint and the one-loader-two-sources
property together. Documented in `apps/asteroids/src/balance.rs`'s header.

## The demo seam review of 2026-09-07 — what it ruled out, and what it did not read

A read-only review of every crate under `apps/` (except `apps/crcbl-sample-test`
and `apps/render-harness`), the `crcbl new` template, `web/demos/*/main.js` and
`web/tools/`, asking what code carries the same knowledge in two or more demos
and belongs in the engine. The owed work it found is in `docs/backlog.md` under
"The demo seam"; this is the half that must not be re-proposed and the half that
was not looked at. The review's own diff counts were taken over comment-stripped
bodies; the parent re-ran four of them (`hud`/`orbit` `gpu.rs`: 8 lines, all the
sample's name; `hud`/`shard` `menu.rs`: 2 lines; the nine golden scripts; the
template's feature list) and they held.

### Already hoisted — do not re-propose

Verified present in the tree:

- **`apps/crcbl-sample-test`** — `SampleRun` (the whole `--screenshot` drive:
  stale-file removal, the `--headless --no-debug-overlay --screenshot`
  invocation, the exit-code and extent assertions, the summary-state check, and
  the `simulation_advanced` tick check), `Block` (`brightness`, `channel`),
  `required_backend`, `adapter_line`. Used by six demos.
- **`crcbl::impl_game_gpu!`, `impl_polled_gpu!`, `impl_polled_bundle!`** in
  `crates/crcbl/src/engine.rs` — the `GameGpu`/`GpuSurface` forwards,
  `PendingGpu` and its `poll`, and both `open` paths routed through one `desc`.
  `apps/lantern` deliberately writes `impl PolledGpu` by hand because it threads
  its forced path through `request_open`; the macro's own docs say so.
- **`crcbl::impl_web_pending!` and `crcbl::web_exports!`** — the whole browser
  lifecycle. `apps/viewer` writes its `WebPending` by hand because its `Options`
  has a field no default can fill; `apps/orbit/src/web.rs` records why.
- **`crcbl::web::{ASSET_BASE, STATUS_*}`** — one definition of the wire format.
- **`web/engine/demo.js`'s `bootDemo`** — boot sequence and rAF loop for all
  eighteen shims. Also `web/engine/{wasm,log,storage,shell,audio}.js`.
- **`web/engine/knobs.js`** — the knob-panel driver alcove's and sundial's pages
  share (hoisted 2026-09-07).
- **`crcbl::args::{seed_u64, seed_u32, assert_shared_help, assert_screenshot_help, assert_forced_path_help}`**,
  **`GpuError::pools`**, **`crcbl::engine::heartbeat_due`** and
  **`crcbl_sample_test::browser_gate_expectation`** — the smaller seam findings
  (hoisted 2026-09-07). A `Heartbeat::every(ticks)` type was considered and
  declined: the `ticks` counter is each demo's own state and the line content is
  each demo's own, so the shared part is the one-line cadence test and nothing
  more.
- **`crcbl::args`** — `Common`, `Consumed`, `Invocation`, `run_front_end`,
  `COMMON_OPTIONS_HELP`, `SCREENSHOT_HELP`, `COMMON_TAIL_HELP`, `positive`,
  `number`, `size`, `HEADLESS_FRAME_BUDGET`, `MAX_TICK_RATE`.
- **`crcbl::store::record::Record`** — the platform arms, the encode, the
  corrupt-file case (the residue is finding 14).
- **`crcbl-golden`** — `Image`, `Golden`, `compare`, `Tolerance`, `srgb`.
- **`crcbl::render::menu_skin` / `UiRenderer` / `RenderGraph` / `TransientPool`
  / `PassTimers` / `ForwardRenderer::present_target`**.
- **`crcbl-greybox`** — the primitive kit and `scene3d()` with its `GREYBOX_*`
  slots.
- **`tools/nextest-summary.sh`, `tools/vk-validation-log.sh`,
  `crates/crcbl-vk/tests/vulkan-icd.sh`** — sourced by all nine golden
  harnesses.
- **`horde/src/controls.rs`'s shared parts** — `TouchStick`, `PauseControl`,
  `CONTROL_STYLE` are already engine.

---

### Considered and ruled out — same shape, different knowledge

- **The 2D sprite `gpu.rs` bundle (asteroids / breakout / flappy / horde /
  sparks).** Comment-stripped, `asteroids` and `breakout` differ by 127 of ~300
  lines: `breakout` holds `paddle_x`/`ball`/`bricks` where asteroids holds a
  `RenderState` and an `alpha`, its camera is fitted to a fixed field where
  asteroids applies `TEXELS_PER_UNIT`, and their `Scene::build` signatures
  differ outright. `asteroids` vs `flappy` is 178 lines. This is what
  `docs/notes/samples.md`'s "The two sample `gpu.rs` files that stopped being
  identical" predicted, and its decline still holds: the helper would need a
  flag per caller. **Already declined — not re-proposed.** (Finding 2 is a
  different bundle: no camera, no sprite pass.)
- **`apps/breach/src/camera.rs` vs `apps/puppet/src/camera.rs`.** Opposite
  signs, and the pair is deliberate evidence that the conversion is the rig's.
  **Already declined** in `docs/notes/samples.md`; see finding 15 for the part
  the decline does not cover.
- **`crcbl_ui::hud`'s `Hud`/`HudPanel` for the 2D samples' HUDs.** **Already
  declined** in `docs/notes/samples.md`: `Label` has no per-label colour and
  every sample draws three colours in one panel.
- **`HudStrings` / `draw_hud`.** The cache keys and the strings are each game's
  content. **Already declined.**
- **`with_shell` / `open_the_window`.** **Already declined** with stated
  reasons.
- **`DebugModule` impls per demo** (`FieldStats`, `BoardStats`, `CourseStats`,
  `HudStats`, `Stats`, …). Same shape, genuinely different numbers. **Already
  declined.**
- **The `bind: (ex) => ({ prepare, boot, frame, … })` block in every
  `web/demos/*/main.js`.** Eighteen copies of ten forwards, and it must stay:
  `web/tools/check-exports.mjs` scans the shim for `.__crcbl_…` literals to
  learn which exports to require of the artifact, and a template literal would
  hide every one of them. The reason is written into each shim's own header.
  Ruled out on the code's own evidence.
- **Mesh and material slot constants** (`towers::map::GROUND_MESH`,
  `breach::map::FLOOR_MESH`, `shard::zone::SLAB_MESH`, …). Same _pattern_ —
  hand-numbered indices into a `SceneDesc` — but each set names that demo's own
  geometry, and `crcbl-greybox` already publishes `GREYBOX_*` for the shared
  kit. Ruled out.
- **`apps/bare`.** Its module doc states it "must never be converted to the
  engine-owned loop — a guard that adopts the thing it guards against is not a
  guard". Every resemblance to the other demos is the point. Ruled out.
- **`apps/sandbox`'s `args`/`Options`.** `crcbl::args`'s own module doc names
  sandbox as deliberately not a consumer: "A design that could not accommodate
  it would be a design that had guessed." Ruled out.
- **The `parse()` loop skeleton** (18 copies of the `while let Some(arg)` /
  `match options.common.consume(...)` shape). `crcbl::args`'s module doc calls
  this "Offered, not imposed" and argues a game must own its parse loop, its
  error messages and its ordering. The _arms_ inside it are candidates (finding
  9, and `--seed`); the loop is not. Ruled out.
- **`fn grouped(value: u64)`** — the thousands separator in
  `apps/sparks/src/page.rs`. One copy. Not a finding.
- **Balance loading (`apps/asteroids/src/balance.rs`), the save payload
  (`apps/shard/src/save.rs`), loot RNG salts and level tables
  (`apps/shard/src/{loot,level}.rs`).** One consumer each — `grep -l` for
  `SaveWriter`/`AutosaveRing`/`save_ticks` returns `apps/shard` only, and
  `--balance` appears in `apps/asteroids/src/args.rs` only. Nothing to share
  yet; they are what a _second_ consumer would trigger.
- **`HEARTBEAT_TICKS`'s value.** 60 / 30 / 15 across fourteen demos — a
  per-sample tuning knob, not one fact. Only the gate around it is shared
  (finding 17).

---

### DECIDED 2026-09-10 — the help asserts stay in `crcbl::args`

`assert_shared_help`, `assert_screenshot_help` and `assert_forced_path_help` are
called only from `#[cfg(test)]` modules, and moving them down to
`apps/crcbl-sample-test` was declined. `apps/bare` is the reason: its charter is
to drive the engine "using nothing but `crcbl`'s public API and its own
`loop {}`", it carries no dev-dependencies at all, and it asserts the same help
blocks as every other demo. A move would either put a sample-support crate in
bare's dependency list, against the one thing that sample exists to prove, or
leave bare hand-writing the asserts the hoist deleted everywhere else.

The cost of leaving them is three `pub fn`s in the shipped library that only
tests call. They sit beside the constants they check, which is the only place
they could be checked from without re-exporting those constants, and the
alternative costs more than it saves.

### DECIDED 2026-09-10 — `scripted` and `headless` stay hand-written

A `crcbl::scripted_loop!` macro over the `#[cfg(test)]` `scripted` helpers —
every demo but alcove, options, sparks and sundial has one — was declined. Read
three of them and they are three different bodies under one name: hud's is the
plain `with_shell` call, horde's builds the engine at its title screen and then
starts the run, and lantern's wraps the loop in a `Scripted` struct that also
holds a `crcbl::debug_view::for_test()` guard. The `headless` half is the same
story through the type system — `Options` is each sample's own struct, so a
shared builder cannot name its type.

What they share is the shape, not the knowledge, and a macro covering them would
need a clause per sample plus a `carry:` for lantern's guard: the same reasoning
that keeps `apps/viewer`'s `PendingLoop` hand-written. A demo that wants the
pair and has none — `apps/options` is the one a coverage gap is waiting on —
writes the same three lines rather than adopting a macro.

### DECIDED 2026-09-10 — the readout panel's coverage is unit tests and browser rows

The five 3D demos' goldens do not read the panel, and the seam review's claim
that shard's did is wrong: `apps/shard/tests/golden.rs` builds a
`crcbl::screenshot::ForwardScene` through `OffscreenSetup` and runs no UI pass
at all, so sabotaging `ReadoutPanel::draw_at`'s right-alignment arithmetic left
`run-shard-golden.sh` passing every comparison on radv.

**A golden that includes the overlay was declined.** The offscreen screenshot
path is a scene path: adding the panel to it means wiring a UI pass into
`crcbl::screenshot`, which is an engine change to the harness every 3D golden
shares and its own task rather than a coverage decision. What holds the panel
instead is each page's own layout test, named
`every_reading_is_laid_out_where_it_can_actually_be_seen` in breach, puppet,
shard and sparks and `the_page_keeps_to_its_own_column` in towers; two of them
were the tests that did redden when that sabotage was measured. The five browser
rows read the panel's pixels in a real browser besides. Two independent readings
of the same claim is the coverage; a third costing an engine change is not worth
it.

### Where the hoists landed, and what each one ruled out (2026-09-07)

Six records from landing seam slices 1–5. Each says why a symbol is where it is
rather than where the review proposed, or what was declined on the way, so
neither gets re-proposed.

#### The pause menu and the page bundle live in `crcbl::engine`, not where the seam review put them (2026-09-07)

The 2026-09-07 seam review proposed `crcbl::ui::menu::pause_menu` and a
`PageBundle` in `crcbl-render`. Neither is reachable: `crcbl::ui` _is_
`crcbl_ui` (`pub use crcbl_ui as ui`), and `RESUME_ID` and its neighbours are
defined beside `MenuAction` in `crates/crcbl/src/engine.rs`, which `crcbl-ui`
cannot see; `PageBundle` holds a `GpuContext` and returns `GpuError`, both of
which are `crcbl`'s and not `crcbl-render`'s. Both landed in
`crates/crcbl/src/engine/`, beside `pause.rs` and `console_button.rs`, which are
the same kind of hoist. Moving the widget ids down into `crcbl-ui` was
considered and declined: their doc comments link `PAUSE_KEY` and
`MenuAction::from_id`, which would become unresolvable and red `cargo doc`.

#### The screenshot arming reaches the context through a second trait (2026-09-07)

`crcbl::engine::HoldsContext` exists because `GameGpu` could not carry
`context_mut`: `engine.rs`'s own `FakeGpu` and `BareGpu` hold no `GpuContext` at
all, so a required method there is one they could only answer by lying. The
sixteen inherent `context_mut` accessors were therefore **not** deleted —
`impl_game_gpu!` forwards to them, which is the pattern that keeps
`unconditional_recursion` working — but their five-line rationale collapsed to
one pointer at the trait. **Considered and declined:** putting the screenshot
request on `GpuOptions` so `GpuContext::open` arms it itself, which would delete
the accessors and the free function both. It is the better shape and it touches
`GpuOptions`, `GpuContextDesc`, both bring-up paths and `crcbl new`'s scaffold,
so it is its own task.

#### `impl_game_gpu!` now requires `context_mut`, which quarry and viewer gained (2026-09-07)

Both hold a `ctx: GpuContext` and neither arms `--screenshot` in its `assemble`;
the accessor exists only for the macro's forward. That they do not arm it is a
pre-existing gap — `apps/quarry` and `apps/viewer` are the two samples with no
golden-harness screenshot — and was not fixed here.

#### `apps/viewer`'s `PendingLoop` stays hand-written (2026-09-07)

`crcbl::impl_pending_loop!` covers the other seventeen. Viewer's pending state
carries an `Rc<Model>` beside the options and its `request` takes a fourth
argument, so a `carry:` clause would put one sample's exception into every other
invocation. Recorded so it is not re-proposed; the same reasoning is written on
the struct and in the macro's docs.

#### `crcbl::knob::Knob` lives in the umbrella, and why not lower (2026-09-07)

The 2026-09-07 seam review proposed the knob beside
`crcbl::render::console_table()` or in `crcbl-console`. Neither works.
`crcbl-console`'s Cargo.toml charter is "**No dependencies at all**, on purpose"
(debug-console decision 1 in `docs/notes/tooling.md`), so it cannot log a
refused write — and reporting the refusal rather than dropping it is the one
behaviour both samples' `set` had. `crcbl-render` is wrong the other way: the
knowledge is about `ConVar` and `Table`, not rendering, and there are three
console tables (`crcbl`, `crcbl_core`, `crcbl_render`), so `Knob::named` takes
the table as an argument and a knob in render could not name the other two. It
landed in `crates/crcbl/src/knob.rs`, the same place the pause menu and
`PageBundle` went for the same kind of reason.

Its tests are `crates/crcbl/tests/knob.rs` and not a `mod tests`:
`crcbl_console::guard::declared_names` holds `crcbl::console_table` to every
`convar!` under `crates/crcbl/src`, so fixture variables declared beside the
code would be ones the engine's own table is then required to publish.

#### A file under `web/engine/` must name no sample's export, even in prose (2026-09-07)

`web/tools/check-exports.mjs` scans the _shared_ half of the shim
(`web/engine/`) for `ex.__crcbl_…` and requires every symbol it finds there of
**every** demo's artifact. A doc comment in `web/engine/knobs.js` naming
`__crcbl_alcove_technique` failed `web/build.sh` on breakout. Recorded so the
next shared module does not trip it.

### Coverage — what I did not read

I read whole files where a finding rested on them and extracted specific items
elsewhere. Stated plainly, I did **not** read:

- **Game logic**: `apps/horde/src/game.rs` (8577 lines),
  `apps/asteroids/src/game.rs` (5190), `apps/breakout/src/game.rs` (3292),
  `apps/flappy/src/game.rs` (2486), `apps/breach/src/game.rs` (2230),
  `apps/shard/src/game.rs` (2147), `apps/orbit/src/game.rs`,
  `apps/puppet/src/game.rs`, `apps/towers/src/game.rs`, `apps/hud/src/game.rs`.
  A cross-demo simulation finding could be hiding there and I would not have
  seen it.
- **`apps/viewer` almost entirely** — `app.rs` (4135), `gpu.rs` (2103),
  `demo_model.rs`, `model.rs`, `anim.rs`, `listing.rs`, `shelf.rs`,
  `fixture.rs`, `watch.rs`, `web.rs`. I only checked its
  `ui_text`/`row_value`/`poll` against the others. Viewer's `gpu.rs` is the
  largest in the tree and I did not compare it to anything.
- **Content modules**: `apps/lantern/src/room.rs` (3010),
  `apps/sundial/src/plaza.rs` (1753), `apps/puppet/src/map.rs` (1807),
  `apps/alcove/src/court.rs` (1389), `apps/shard/src/zone.rs`,
  `apps/breach/src/map.rs` and `map/practice.rs`, `apps/towers/src/map.rs`, all
  five `art.rs`, `apps/breakout/src/scene.rs`,
  `apps/quarry/src/{face,tile,dag,scene}.rs`,
  `apps/sparks/src/{stage,show,effects}.rs`,
  `apps/bracket/src/{rating,sim,queue}.rs`,
  `apps/towers/src/{creep,path,tower,wave}.rs`,
  `apps/shard/src/{foe,loot,light,save}.rs` beyond their constants,
  `apps/puppet/src/{rig,anim}.rs`, `apps/alcove/src/occlusion.rs` and
  `apps/sundial/src/filter.rs` beyond the nine helper functions.
- **`apps/options` beyond `gpu.rs` and the arming block** — `menu.rs` (1023),
  `view.rs`, `audio.rs`, `app.rs` (2781). The settings screen is the one demo
  whose menu is not a pause panel and I did not evaluate it for shared knobs.
- **Audio**: I confirmed the `plays`-counter duplication by symbol listing, not
  by reading `apps/{asteroids,horde,options}/src/audio.rs` in full. The backlog
  already owns that finding.
- **The four large golden suites**: `apps/sundial/tests/golden.rs` (4033),
  `apps/alcove/tests/golden.rs` (1909), `apps/lantern/tests/golden.rs` (1863),
  `apps/shard/tests/golden.rs` (651), and all of `apps/quarry/tests/device/*`
  (~2600 lines across nine files) and `apps/alcove/tests/run-alcove-views.sh`
  (710). Finding 12's claim-shape duplication is asserted for the five small
  suites I read and _not_ verified for these.
- **`web/tools/browser-e2e.mjs`** — 512 KB; I read roughly 400 lines around the
  shard and breach expectation blocks and listed its top-level constants. Other
  Rust↔JS mirrors almost certainly exist in the parts I did not open.
- **`web/tools/{gpu-replay,probe-groups,stream-decode,reply-encode}.mjs`** and
  `web/engine/{gpu-probe,gpu-replay,gpu-stream}.js` — none of them demo code,
  all unread.
- **`web/build.sh`, `web/run-browser-e2e.sh`, `web/pages/*.html`,
  `web/templates/`** — unread, so my claim that a new `knobs.js` under
  `web/engine/` needs no build change (finding 11) is an inference from
  `web/build.sh` copying `web/engine/` wholesale, which I did not confirm.
  (Confirmed 2026-09-07 while landing it: `assemble_static()` walks `web/`
  pruning only `tools`, `jobs`, `pages`, `templates`, `*.sh` and `README.md`.)
- **`.github/workflows/ci.yml`** — I grepped it for the nine golden `run:` lines
  and read nothing else, so the "which gates this touches" notes are from those
  greps plus the demo lists, not from reading the workflow.
- **`docs/backlog.md`** — 16,982 lines; I searched by symbol and by the topic
  words the brief suggested and read six sections. An entry phrased in words I
  did not guess would have been missed.
  `docs/notes/{backends,browser,ci,process, rendering,simulation,tooling}.md`
  were searched by symbol only; `docs/notes/samples.md` I read in the four
  relevant sections.
- **`docs/plan/sample/*.md`** — not read; the "which docs name the moved
  symbols" notes come from `grep -rl` over `docs/`, which finds a path but not
  whether the prose around it would also go stale.
