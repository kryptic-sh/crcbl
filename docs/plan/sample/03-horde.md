# Sample 03 — horde

Survivors-lite: one player, auto-firing weapon, thousands of enemies converge,
survive N minutes. The renderer-at-scale sample — the stage 3 "flat CPU cost"
claim, demonstrated as gameplay instead of a synthetic sandbox scene.

## Proves

- GPU-driven pipeline under gameplay conditions: 5–10k live enemies +
  projectiles, all through instance deltas + GPU culling + indirect draws.
  Synthetic sandbox scenes are static-ish; horde adds per-tick movement of
  everything — the dirty-range delta upload path gets its real workload.
- Server tick at scale: the ECS SoA claim (linear iteration, cache-friendly)
  measured with 10k-entity systems. If the server can't tick 10k simple agents
  at 60 Hz, that's a stage 4 finding.
- Replication at scale: snapshot size/bandwidth with 10k entities forces the
  question interest management answers post-MVP — this sample produces the
  numbers that justify (or defer) that work.
- Profiler HUD as the primary dev instrument: this sample is built _by watching
  the profiler_, and its doc records the measured budgets.
- **Physics slice it drives** (interleaved build): broadphase + overlap queries
  at 10k-body scale — batch query API, BVH refit cost, island/ sleeping
  pressure. Physics perf numbers recorded here alongside render numbers.

## Scope

- One arena (flat plane + props), one player, WASD + auto-aim weapon.
- Enemies: 2–3 types, dumb seek movement + separation (cheap flocking-lite, no
  pathfinding), contact damage, HP, on-death despawn. Separation neighbors +
  contact damage = `crcbl-phys` broadphase overlap queries; player weapon =
  segment/swept CCD.
- XP pickups + one "pick 1 of 3" level-up choice screen (exercises game UI
  mid-session) — but a small fixed upgrade pool.
- Timer, kill count, death screen. 5-minute survival target.
- **`.crpix` sprites for the player, the enemy types and the pickups** (sample
  rule 11), and **the debug panel on** (rule 4) — which this sample needs more
  than any other on the ladder, because its whole claim is a flat CPU cost at
  10k instances and the panel's frame-timing module is where that is read. The
  arena and props follow whatever P9's asset path provides; the actors are
  sprites. Ten thousand enemies drawn from a handful of sheets is also the first
  time `SpriteRenderer`'s per-sheet batching is under real pressure, which is a
  finding either way.

## Non-goals (hard cap)

Meta-progression, many weapons/characters, bosses, terrain, pathfinding,
particles beyond reused debug-draw-style primitives. This is a benchmark wearing
a game costume — keep the costume thin.

**"Terrain" here means simulation terrain** — heightmaps, traversable geometry,
anything the seek loop has to reason about. It was never meant to bar decoration
drawn under a flat arena, and on 2026-08-04 an art overhaul was asked for that
adds a tiled grass ground and scattered props. The line that matters is the one
next to it: **pathfinding stays out**, so the props block the player and nothing
else, and the horde still seeks in a straight line at a cost that does not move
with the size of the field. A prop the enemies had to route around would be
pathfinding wearing a tree costume, and that is the cap doing its job.

## Milestones

1. 1k enemies seeking player, culling stats on HUD (stage 6-ish start, scene
   from file).
2. Combat loop complete (damage, XP, level-up UI) — after stage 7.
3. Scale push: raise counts until a budget breaks; file engine findings; record
   the numbers in this doc. **Done — see "The scale push (milestone 3),
   measured" below.** The budget that broke is the tick's, not the render's.

## Exit criteria

- 10k enemies at 60 fps render / 60 Hz tick on the reference Linux machine
  (numbers recorded here, revisited per backend in stages 9–10 — the browser
  gets its own smaller recorded budget).
- CPU frame time demonstrably flat 1k → 10k on the render side (profiler capture
  archived in the doc).
- Playable and mildly fun for 5 minutes — fun is not the goal but "not obviously
  broken as a game" is.

## Where this stands

**Slices 18a, 18b and 18c have landed** (`apps/horde`). One arena, one player
with WASD movement and a gun that aims itself, three enemy kinds with seek plus
separation, contact damage, hit points, death and restart; `.crpix` art through
`SpriteRenderer` with `SampleMode::Pixel`; XP gems that drop where an enemy
died, health potions that a brute leaves behind now and then, and a "pick 1 of
3" level-up from a fixed pool of six upgrades; pause, level-up and death menus
over the shared `crcbl_render::menu` art, with the debug panel on; a tiled grass
ground under all of it with trees and bushes scattered over it; six spatial
cues, the longest run kept between sessions, and the browser demo at
`https://crcbl.kryptic.sh/demos/horde/`.

**The shipped default enemy cap is `DEFAULT_MAX_ENEMIES`, not the ten thousand
this doc's exit criteria name**, and `--max-enemies` is why raising it needs no
rebuild. That constant's own doc in `apps/horde/src/game.rs` argues the decision
and is the place to change it; the measurements below are all taken with
`--prefill`, which stages a field directly rather than waiting on a spawner that
would take ten minutes to reach ten thousand.

**The potion is a second `PickupKind`, not a second population.** Both pickups
share one `Vec`, one entity index, one `MAX_PICKUPS` ceiling and one trigger
collider, which is what leaves the leak test's two exact equalities and its
entity growth bound saying exactly what they said before. `game::drops_potion`
deals the drop from a `LOOT_HAND` salt on the run seed indexed by the kill
counter, so a potion is the same event in a replay as in the run it replays. The
rate — one brute in twenty, and a brute is a tenth of the spawn table — was
settled by measurement: at one in three the kiting soak stopped reaching a death
at all, which is contact damage no longer being what the run is about.

**The props block the player and nothing else**, which is the hard cap above
being enforced rather than an unfinished half of the feature.
`game::scatter_props` deals a jittered lattice of trees and bushes from the
game's seed — the _game's_, not the run's: a restart re-deals the horde and
leaves the arena where it was, because a player who learns where the cover is
should keep that between attempts. They are `PropView` in a plain `Vec` with no
entity and no collider, so the horde's `N` overlap queries per tick return
exactly what they returned before and the leak test's two exact equalities still
account for the whole world. `game::push_out_of_props` runs once, on the player,
inside the same pass as the arena clamp, and slides rather than sticking.

**And a start screen, which was argued against and then asked for.** The slice
that built the menus deliberately shipped without one: this game's board is
empty at `t = 0` and fills up because time passes, so a waiting state is a blank
arena with a prompt on it, and the sample rules do not require one. The user
played the demo and asked for the screen — _"the horde game autostarts, instead
of showing the start screen"_ — which settles it. `GameState::WaitingToStart`
short-circuits `run_tick` on its second line, so nothing spawns, nothing moves
and the run clock does not start until `R` or `Space` is pressed; `restart` now
lands on that screen rather than in play, as asteroids' and flappy's do. The
reasoning is kept in `apps/horde/src/game.rs` so nobody re-derives it and
re-implements the autostart; the reversal is the user's call and it is final.

**The art is a sheet per subject and that is the sample's own decision.**
Everything numerous — the player, all three enemy kinds and both pickups — is in
one `assets/actors.crpix` at one frame size (34 texels, which is the brute's
collider box at 20 texels a unit), so the whole field is a single
`SpriteRenderer` batch **whatever order it is emitted in** and `art::Scene`
needs no grouping pass over the crowd. Asteroids has three rock sheets and has
to emit largest-first to hold three batches; a field of ten thousand walked in
the order the game holds it would be ten thousand. The shot is a second sheet,
because it is 8 texels and would otherwise be drawn in a quad twenty times its
own area, and `assets/terrain.crpix` — the tiled grass ground, added after these
measurements — is a third, with `assets/props.crpix` a fourth at 36 texels. Both
of the last two are a sheet of their own because a `.crpix` declares one frame
size for the whole file. The price is the transparent margin round the small
kinds — a runner is 13 texels of art in a 34-texel quad — and it is bounded by
the screen rather than by the horde. **18c measured both halves and both hold**;
see "The batching claim, measured" below, including what the ground did and did
not change about it.

**The level-up freezes the field**, and the freeze is simulation state rather
than a loop pause: the choice changes what the simulation does, so a seeded
script has to replay it. `GameState::LevelUp` short-circuits `run_tick` and
`freeze_field` writes a zero velocity to the player, every enemy and every bolt
**once**, on the tick the screen opens — so nothing moves for as long as it is
up and no branch is added to the hot path. Bolts keep their velocity so it can
be handed back; enemies do not need to, because `steer_enemies` writes them a
fresh one on the first tick after.

## The scale push (milestone 3), measured

**This is the exit measurement.** Everything below was taken on the reference
Linux machine with the release binary, and the conditions are stated per table
because they are not the same conditions.

### The machine, and what it could and could not be run on

| What    | Which                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| CPU     | AMD Ryzen 9 9950X3D, 32 threads. **Single-threaded**: there is no `crcbl-jobs` (P8) and no parallel ECS schedule, so every number here is one core.                                                                                                                                                                                                                                                                                                            |
| GPU     | AMD Radeon RX 7900 XTX, radv (Mesa 26.1.6). `lavapipe` is installed and was not used.                                                                                                                                                                                                                                                                                                                                                                          |
| Build   | `cargo build --release -p horde`.                                                                                                                                                                                                                                                                                                                                                                                                                              |
| Window  | **There is none.** The build environment has no `DISPLAY` and no `WAYLAND_DISPLAY`, so every run is `--headless`, which gives `crcbl-vk` an **offscreen image ring** at 960 × 720 rather than a swapchain on a surface. It is the same acquire → record → submit → present code path — `SurfaceTarget::Offscreen` exists so that it is — but it is **not** a windowed present and it is not vsynced. The windowed native path is still compiled and never run. |
| Browser | A separate measurement was not taken. `web/run-browser-e2e.sh` runs the demo under Chromium's SwiftShader, which measures the software rasteriser rather than the browser. The browser gets its own recorded budget when there is a machine with a real browser GPU to take it on; the exit criteria already treat that as a separate number.                                                                                                                  |

The fixture is **`--prefill N`**, which stages `N` enemies over the whole arena
on a grid before the first frame. The spawner ramps from one enemy every half
second to one every sixteenth, so reaching ten thousand by playing is over ten
minutes that nothing survives; and staging them at the 1.25 units separation
settles at — which is what the 18a fixture did — needs 125 × 125 units for ten
thousand against an arena of 96 × 72, so most of that field lands on a wall
under `clamp_to_arena`. Falsified, not argued: swapping the arena-fitted grid
for a 1.25-unit one collapses 5 280 of the ten thousand onto shared positions,
and `a_prefilled_field_is_the_size_and_shape_it_was_asked_for` goes red saying
so. At ten thousand the fitted grid is **0.82 units apart**, which is what ten
thousand in this arena actually looks like.

**`--prefill` starts its own run**, since the start screen landed: a prefilled
field left waiting would time a `run_tick` that returns on its second line and
report the result as the cost of ten thousand enemies. The loop queues the start
edge into the same action map a key press goes to, and
`a_prefilled_run_does_not_wait_at_the_title_screen` is what says so. The numbers
below are re-measured on the build with the screen: the prefilled rows play (the
start edge fires), and the `field 0` row never starts — the fixture offers no
way to start a non-prefilled run — so it measures the title screen, which is why
its `drawn` is the 305-strong constant and its CPU carries the menu.

### The render side: flat, and not close to a budget

Conditions: release, headless, `--backend vk` on radv (RX 7900 XTX), 960 × 720
offscreen ring, `--wall-clock --fps 0 --tick-hz 1 --frames 20000`. The tick rate
is one hertz so that the run — which takes about two seconds of wall time —
contains no more than two ticks and the frame being measured is the **render
path alone**. `--fps 0` is the frame limiter being asked to stay out: it landed
after the first version of this table, and it sleeps **inside** the measured
frame, so a paced run's CPU column would read the limiter rather than the frame.

- **CPU** is the debug overlay's own frame-timing module (`FrameStats`), the
  mean over its rolling 120-frame window, driven by the real monotonic clock.
  `--wall-clock` exists for this: a headless run's clock is a fake one stepping
  exactly 1/60 s, so without it the panel reports 16.667 ms at every count and
  measures nothing. `--fps 0` keeps the limiter out of the same number — a paced
  run's `render_dt` includes the limiter's sleep, which is why the first
  re-measurement read 1.050 ms at every count.
- **GPU** is `PassTimers` — real timestamp queries on the same device. It is
  **one resolved frame's** timestamps (`PassTimers::latest`), not an average,
  taken from frame 19 997. Three repeats of each row agreed to the microsecond,
  except the 5000 row: its frame's on-screen state (a bolt in the air or not,
  the death position) and one noisy run moved the CPU read across 0.109–0.277 ms
  and the sprites pass across 0.016–0.031 ms, so that row reports the quiet
  repeat with the range noted beside it.
- The two columns are not addable. The GPU work overlaps the next frame's CPU.

|  field | drawn | batches | CPU frame | `arena` GPU | `sprites` GPU | `menu` GPU | `ui-composite` GPU |
| -----: | ----: | ------: | --------: | ----------: | ------------: | ---------: | -----------------: |
|      0 |   305 |       3 |  0.107 ms |    0.005 ms |      0.009 ms |   0.012 ms |           0.004 ms |
|  1 000 |   554 |       4 |  0.096 ms |    0.005 ms |      0.009 ms |          — |           0.005 ms |
|  2 000 |   801 |       4 |  0.109 ms |    0.005 ms |      0.012 ms |   0.009 ms |           0.004 ms |
|  5 000 | 1 555 |       4 |  0.109 ms |    0.004 ms |      0.016 ms |   0.008 ms |           0.003 ms |
| 10 000 | 2 750 |       4 |  0.123 ms |    0.005 ms |      0.027 ms |   0.009 ms |           0.004 ms |

Re-measured 2026-08-07 with `assets/terrain.crpix` and `assets/props.crpix` in
the picture; the first version of this table predates both. Two rows carry a
second `sprites`-labelled pass, the menu — the run never starts at `--prefill 0`
(the fixture offers no way to start a non-prefilled run, so the row is the title
screen), and at 2000 and above the player dies at about a second and the death
menu is up at the measured frame. The `sprites` column is the field pass alone;
the `menu` column is the second one. `—` means the pass was empty and skipped.

**The `<0.5 ms GPU at 1080p` criterion from `07-ui-debug.md` is now measured,
not argued.** The 1080p extent was unreachable headless until `--size` landed;
with it, the same fixture runs at 1920 × 1080: release, `--backend vk` on radv
(RX 7900 XTX), headless offscreen ring,
`--wall-clock --fps 0 --tick-hz 1 --frames 900 --prefill 10000 --size 1920x1080 --debug-overlay`.
Three repeats read a 0.065–0.068 ms total GPU frame (`ui-composite` 0.005 ms —
the panel's own pass, unchanged from its 0.005 ms at 960 × 720) and a
0.150–0.153 ms CPU frame mean. The 2.25× pixel increase moved the field sprite
pass from 0.020 to 0.041 ms, which is where the total's growth from 0.038 to
0.067 ms comes from; the criterion's budget has two orders of magnitude of
clearance at the extent it names, and the numbers come from the engine's own
exit log (`gpu passes` / `frame cpu` lines — the GPU line was `frame timing`,
one latent frame, until 2026-08-28 replaced it with a p50/p95 per pass), so the
run is reproducible without instrumentation. Measured 2026-08-07.

`drawn` is what survived the CPU view cull and reached the pass — the arena is
96 × 72 units against a view of about 37 × 28, so most of a large horde is off
screen and the number on screen is bounded by the **screen** rather than by the
field. It counts the whole 305-strong constant — the 300 ground tiles, the 4
props and the player, which the view always holds — plus whatever of the horde
the view shows, which is why it reads 305 at an empty field and why the actor
count is `drawn − 305`. `field`, `culled`, `ground`, `props`, `drawn` and
`batches` are a `scene` section this sample adds to the debug panel, so they are
readable in the running game and not only from a test. `ground` and `props` are
reported beside `field` rather than inside it: the tiles are generated from the
view and the scenery is a constant of the seed, so folding either into `field`
or `culled` would put a constant into the two numbers that exist to show what
moves with the horde.

**The exit criterion "CPU frame time demonstrably flat 1k → 10k on the render
side" is met, and by a wide margin.** 0.096 ms at one thousand and 0.123 ms at
ten thousand: the whole marginal cost of nine thousand more enemies is **27 µs a
frame**, or 0.16 % of a 16.67 ms budget. The render path carries ten thousand at
something like 8 100 frames a second and would carry ten times that before the
frame budget noticed.

The same series with `--backend vk` swapped for `--backend null` isolates the
game's own CPU from the driver's: **0.008 ms at zero enemies and 0.033 ms at ten
thousand**, so the game's share is 2.5 ns per field enemy — the `RenderState`
copy, the cull's four comparisons and the instance build — and the other 0.09 ms
is command recording, submit and present, which does not move with the field at
all.

### The batching claim, measured

**It holds.** `batches` is 4 at every populated count in the table above and 3
when no bolt happens to be in the air, and it does not move between one thousand
and ten thousand. Pushed harder than the running game can: the
ten-thousand-visible-enemies test packs the whole ten thousand _inside_ the view
so nothing is culled, interleaves the three kinds in the order `swap_remove`
leaves behind, and asserts the count over every sprite in the frame — the ten
thousand, the shots, the player, the whole ground lattice and the scenery. It
goes red one higher if the shots are moved onto the crowd's layer.

**The number is 4, and the table above reads it.** `assets/terrain.crpix` and
`assets/props.crpix` each put their subject on a sheet and a layer of its own,
so a populated frame is terrain, then props, then actors, then the bolt —
re-measured 2026-08-07 with both in the picture, and the 1000 row agrees with
the 10000 row. **The claim was never the number**: it is that the count is flat
in the size of the horde, which is why `SceneStats::batches` is in the debug
panel at all. A sheet added for a new subject adds a constant; what would break
the claim is emitting one sheet more than once, which is what
`an_interleaved_field_of_every_kind_is_four_batches` and
`ten_thousand_visible_enemies_are_still_four_batches` are there to catch. Ten
enemies and ten thousand are the same four draws.

**The caveat this section used to carry is gone.** The count was `art::batches`,
a **mirror** of `sprite_pass`'s rule kept in the sample, because
`SpriteRenderer` exposed no batch count and `crates/` was out of that slice's
write scope. It is `crcbl::render::sprite_pass::batch_count` now — the pass's
own answer — so the number would notice the engine's batching rule changing
underneath it. `a_batch_is_a_run_of_one_sheet_and_not_a_distinct_sheet_count`
still pins `A A B A` = 3, which is the case a distinct-sheet count gets wrong
and which this game's own frames cannot distinguish.

### The fill margin: visible in a pass timing, and irrelevant to the budget

**Visible.** The `sprites` pass goes 0.009 → 0.027 ms from an empty field to a
full screen of the crowd, against a full-screen `arena` clear that is 0.005 ms
flat. At 960 × 720 a world unit is 25.71 screen pixels, so the shared 34-texel
quad is 43.7 pixels square, and the 2 445 actor quads at ten thousand (the
table's 2 750 drawn minus the 305-strong ground/props/player constant) are
**4.67 megapixels — 6.8 times the framebuffer** — of blended fill. The pass
grows with it, which is the answer to "does the transparent margin show up in a
GPU pass timing": yes. `SpriteRenderer` has no alpha discard, so a transparent
fragment costs what an opaque one costs.

**Irrelevant.** `two_thirds_of_the_shared_frame_is_transparent_margin` measures
the opaque bounding box of each baked silhouette and weights them by the mix
`EnemyKind::from_roll` actually deals (62 % grunt at 18 texels, 28 % runner at
13, 10 % brute at 34): the average enemy fills **31.5 %** of its 34 × 34 quad,
so 68.5 % of that fill is margin. Applied to the 18 µs the pass costs above an
empty field, the whole price of the one-sheet decision at a full screen of the
crowd is about **12 µs a frame — 0.07 % of the budget**. The alternative — a
sheet per kind at each kind's own frame size — buys those 12 µs and costs a
grouping pass over the crowd and an emission order to get wrong. It is not worth
it at any count this sample can reach.

### The simulation side: this is what breaks

Conditions: release, headless, `--backend null` so nothing is drawn, fixed-step
clock so exactly one tick runs per frame, single-threaded. Measured as a
**marginal**: wall time for `--frames 180` minus wall time for `--frames 60`,
over the 120 ticks between them, best of three each — so process start-up and
the prefill's own cost cancel exactly rather than being estimated. The 120
frames of render inside the window are about 4 ms of a 1 759 ms measurement at
ten thousand, 0.2 %.

| enemies | ms/tick, ticks 60–180 | µs/enemy | ms/tick, ticks 480–600 | µs/enemy |
| ------: | --------------------: | -------: | ---------------------: | -------: |
|     500 |                 0.241 |    0.481 |                  0.939 |    1.877 |
|   1 000 |                 0.418 |    0.418 |                  2.158 |    2.158 |
|   2 000 |                 1.190 |    0.595 |                  6.026 |    3.013 |
|   5 000 |                 4.854 |    0.971 |                 26.289 |    5.258 |
|  10 000 |                14.658 |    1.466 |                 84.087 |    8.409 |

**The two columns are the finding.** They are the same field at the same count,
one to three seconds in and eight to ten seconds in, and they differ by five to
six times. The tick is `N` broadphase overlap queries for separation, and an
overlap query's cost is the size of its **answer** — so the tick's cost is a
function of local _density_, not of `N`. The horde converges on the player by
construction; a crowd that has arrived is several times more expensive than the
same crowd spread over the arena. Nothing before this measured a converged
horde, and it is the state the game spends most of its time in.

Against the 16.67 ms budget:

- **Spread, the tick carries ten thousand — just.** 14.66 ms of 16.67, with two
  milliseconds to spare and the render path costing a tenth of one.
- **Converged, it breaks between two and five thousand.** 6.0 ms at two thousand
  and 26.3 ms at five thousand, so the crossing is somewhere near three
  thousand. Ten thousand converged is 84 ms — **five times over budget, a tick
  rate of 12 Hz.**

That supersedes 18a's provisional 8–9k, and it supersedes it in both directions:
18a measured a field spread at 1.25 units regardless of count, which at ten
thousand is a field larger than the arena and therefore a game that cannot
exist, and it never let the crowd converge. The honest answer is a range, not a
number: **this sample carries about 10 000 spread and about 3 000 converged**,
and the plan's single figure was always going to be one or the other.

### The exit criteria, answered

- **"10 000 enemies at 60 fps render / 60 Hz tick."** Render: **yes**, with two
  orders of magnitude to spare. Tick: **no** — spread it is 14.66 ms against a
  16.67 ms budget and passes; converged it is 84 ms and fails by 5×. The
  criterion as written does not say which, and the difference is a factor of
  six, so it needs rewriting rather than answering.
- **"CPU frame time demonstrably flat 1k → 10k on the render side."** **Yes**:
  0.096 → 0.123 ms, and 0.008 → 0.033 ms with the driver taken out.
- **"Playable and mildly fun for 5 minutes."** **No, and not close.** A default
  run — no prefill, the spawner doing its own work — ends in a **death at about
  24 seconds** with 30 kills and 46 things on the field. At `--prefill 5000` and
  above the player dies in **under a second**: ten thousand enemies in a 96 × 72
  arena is 0.82 units apart, which is several of them inside `PLAYER_RADIUS` on
  frame zero, and contact damage is a rate summed over everything touching. The
  plan's target count and the plan's "survive five minutes" cannot both be true
  of this arena. That is a design finding rather than an engine one and it is in
  `docs/backlog.md`.
- **"Profiler capture archived in the doc."** The tables above are it: the CPU
  numbers are the debug overlay's frame-timing module and the GPU numbers are
  `PassTimers`, which are the two instruments the plan names. There is no
  captured _image_ of the panel — there is no display to take one on.

### What the numbers say P7 and P8 have to buy

The roadmap put this sample **after** P7 (GPU-driven rendering) and P8
(`crcbl-jobs` + a parallel ECS schedule) on the assumption that it needed both.
It needed neither to be built, and the measurement says the two phases are worth
very different amounts to it:

- **P8 is the whole of it.** Every millisecond that matters is in the tick, and
  the tick is `N` independent broadphase queries writing `N` velocities that
  nothing else in the pass reads — order-independent by construction, which is
  the easiest possible thing to parallelise. Sixteen cores on a converged ten
  thousand would take 84 ms to something like 6 ms if it scaled, and it should:
  there is no shared mutable state in the pass at all. Before that, two named
  single-threaded wins sit in front of it and neither has been taken:
  `PhysicsSystem::overlap_sphere` returns an owned `Vec` (two heap allocations
  per enemy per tick, 1.2 million a second at ten thousand) and `PhysicsSystem`
  has no `body_mut` (two hash operations to change one `DVec3`). Both are in
  `docs/backlog.md`.
- **P7 buys this sample almost nothing.** GPU culling replaces a CPU cull that
  costs 28 µs at ten thousand; indirect draws replace two draw calls; instance
  deltas replace an upload of 2 750 × 64 bytes. The whole render path is 0.12 ms
  of a 16.67 ms frame, so the maximum P7 can return here is 0.7 % of the budget.
  It is still worth building — for 3D, for the scenes that are not a flat plane
  of 34-texel quads — but **this sample is not the argument for it**, and the
  roadmap's ordering, which put horde behind it, was wrong about which phase
  this sample was waiting on.

## Early scale signal (superseded, kept for the record)

Taken during 18a because it was one command. **Superseded by the tables above**,
which use a fixture that fits inside the arena and which distinguish a spread
crowd from a converged one; this is kept because it is what the sample was
steered by for two sub-slices and because the difference between it and the real
measurement is itself the finding.

Conditions: `cargo test --release`, headless, **simulation only — nothing is
rendered**, single-threaded, AMD Ryzen 9 9950X3D. `N` grunts staged on a
1.25-unit grid — which is the spacing separation settles at, and which at ten
thousand is a field of 125 × 125 units against an arena of 96 × 72, so the top
row of this table describes a board the game cannot produce — then 60 ticks
timed and averaged.

| enemies | ms/tick | µs/enemy |
| ------: | ------: | -------: |
|     500 |   0.418 |     0.84 |
|   1 000 |   0.619 |     0.62 |
|   2 000 |   1.307 |     0.65 |
|   5 000 |   3.848 |     0.77 |
|  10 000 |  18.433 |     1.84 |

What it got right: the shape — flat per-enemy cost through the middle of the
range, and a tick that is `N` broadphase queries plus `N` `HashMap` writes. What
it got wrong: it read the rise at ten thousand as a working set leaving cache,
and the real cause is that its own grid got denser relative to the arena it was
being clamped into. Density, not cache, is what the cost tracks — which the
spread-versus-converged columns above show directly, at a fixed `N`.

## Re-measured with the pool threaded (2026-08-10)

**The table above was taken single-threaded, and it says so in its conditions.**
`apps/horde`'s `steer_enemies` went onto `crcbl_jobs::pool`'s `par_for` after
those numbers were recorded, and nothing re-ran them — so the conclusion drawn
from them, that a converged ten thousand "fails by 5×", has been describing a
configuration the sample no longer runs in.

Re-measured by the same marginal method — release, headless, `--backend null`,
wall time for the later `--frames` minus the earlier, over the 120 ticks
between, best of three — on a 32-thread machine, varying only `--workers`:

| case                     | enemies | `--workers 1` | `--workers 16` | speed-up |
| ------------------------ | ------: | ------------: | -------------: | -------: |
| spread, ticks 60–180     |  10 000 |      6.508 ms |       3.525 ms |    1.85× |
| converged, ticks 480–600 |   5 000 |     11.508 ms |       2.842 ms |    4.05× |
| converged, ticks 480–600 |  10 000 |     37.383 ms |       7.933 ms |    4.71× |

**Every one of these is inside the 16.67 ms budget on sixteen workers**,
including the converged ten thousand the earlier table recorded as five times
over it.

Two things to be careful about before reading this as a straight refutation:

- **It is a different machine.** Single-threaded converged is 37.4 ms here
  against the original 84.1 ms, so this box is about 2.2× the one that produced
  the table. The machine-independent claim is the **speed-up column**, measured
  on one machine with one binary and one flag changed.
- **The prediction was almost exactly right.** That section argued sixteen cores
  should take the converged ten thousand "to something like 6 ms if it scaled,
  and it should: there is no shared mutable state in the pass at all." It
  measures 7.9 ms. The reasoning was sound and the work it predicted has since
  landed.

**What this changes.** The two single-threaded wins that section named as
sitting in front of parallelisation — `overlap_sphere` returning an owned `Vec`,
and `PhysicsSystem` having no `body_mut` — **are both already taken**.
`PhysicsSystem::body_mut` exists, and the steering pass calls
`overlap_sphere_into` with a reused buffer. The four remaining `overlap_sphere`
calls in `apps/horde/src/game.rs` are one per tick or `#[cfg(test)]`, not per
enemy, so the allocation argument no longer applies to the hot path.

So P8's headline claim for this sample — that the parallel schedule is worth the
whole of the gap — is **met by `par_for` alone**, without the ECS schedule
itself running systems in parallel. What P8 still owes this sample is nothing
measurable; what it owes elsewhere is unchanged.
