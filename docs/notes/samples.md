# Samples — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

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

## flappy (`docs/plan/sample/12-flappy.md`)

Nothing owed that I found. The debug panel claim that this doc carried as "still
owed" was false and has been corrected: `HostedGame::debug_sections` in
`apps/flappy/src/app.rs` contributes the course and the audio and no network
module, which is the modularity check the doc wanted. Same correction applied to
`docs/plan/sample/01-breakout.md`, whose single module is the board.

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

`docs/plan/sample/14-quarry.md`'s exit criteria ask for the reduction to be
attributed: "how much of the reduction is instance culling and how much is
cluster culling, because a single total hides which one is working". quarry now
records both, and the answer is **all of it is cluster culling** — the instance
cull keeps 1 of 1 on every frame, because the scene is one instance of one mesh.

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
sprite pass, menu pass, UI pass over `GpuContext`. The stated reason for leaving
it alone was that two 2D games at the same stage resembling each other is not
the same as one piece of knowledge written twice, and that the failure mode
would be a helper with two callers needing a flag per caller. That is exactly
what a scrolling camera would have become. The trigger is unchanged: revisit
when a third game wants the bundle.

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
  wall; two bias slices took the strip 0.60 m → 0.26 m and
  `docs/plan/45-shadows.md`'s seventh decision — the normal offset, 2026-08-28 —
  took the rest. What it left is "The normal offset scallops one silhouette's
  foot" above, and "What the sun's shadow bias still leaves open" below.
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
- **`crcbl::render::MenuRenderer` / `UiRenderer` / `RenderGraph` /
  `TransientPool` / `PassTimers` / `ForwardRenderer::present_target`**.
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
(plan decision 1), so it cannot log a refused write — and reporting the refusal
rather than dropping it is the one behaviour both samples' `set` had.
`crcbl-render` is wrong the other way: the knowledge is about `ConVar` and
`Table`, not rendering, and there are three console tables (`crcbl`,
`crcbl_core`, `crcbl_render`), so `Knob::named` takes the table as an argument
and a knob in render could not name the other two. It landed in
`crates/crcbl/src/knob.rs`, the same place the pause menu and `PageBundle` went
for the same kind of reason.

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
