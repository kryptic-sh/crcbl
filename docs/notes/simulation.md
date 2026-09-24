# Simulation — records

Records kept so they are not re-derived: measurements, investigations, ideas
considered and declined, and lessons. Open work lives in `docs/backlog.md`.

### Cross-target determinism: the CPU side takes `libm` (2026-08-27, decided 2026-09-06)

Decision record; the decision is in `docs/backlog.md`.

This supersedes the 2026-08-30 "no libm on either side" reading recorded below,
which was about shaders; the decision above splits the two sides.

**DECIDED 2026-08-30 — no `libm`.** The policy is the gap survey's §4 rule
(`docs/notes/rendering.md`, _What the deleted 43-render-standards plan left
behind_): a transcendental is cooked into a table on the host or built from
multiplies (`fog::exp_neg`), and never reaches a colour in a shader. Closed
together with the transcendental-policy entry below. **Open, and it is a
decision rather than a task.** `05-physics.md`'s 2026-07-27 correction routes
determinism-bearing math through the **`libm` crate**; `13-audio.md`'s
correction requires **own polynomial approximations plus a CI deny** on std
transcendentals. They are not interchangeable and neither is built. No workspace
crate names `libm` — it reaches `Cargo.lock` only through `naga` and
`num-traits`, neither of which is in a sim path — and there is no deny anywhere.

**The question to answer:** take the new dependency (`libm`, the user's call per
the dependency rule), or hand-roll approximations with golden values from the
specification. Either way `12-testing.md`'s verification rules apply.

### The transcendental policy is decided; the deny mechanism is not built (2026-08-27, decided 2026-09-06)

Decision record; the decision is in `docs/backlog.md`.

The text below was written while the policy was open; it is kept for the
argument, not for its verdict.

**DECIDED 2026-08-30 — one policy, the cooked-table rule.** Topic 43 §4's rule
(now in `docs/notes/rendering.md`) is the workspace policy: tables cooked on the
host, multiplies in the shader, no libm on either side. The conflict is resolved
by choosing this side; nothing further to build. **Not built, and it needs a
decision.** `13-audio.md` requires own polynomial approximations plus a CI deny
on std float transcendentals; `05-physics.md` requires the `libm` crate. Neither
exists: no `libm` in any manifest, no polynomial approximations, and
`crcbl-audio` calls `powf`, `sin`, `exp` and `cos` today (`spatial.rs`,
`synth.rs`).

**Decision needed:** own polynomials plus a deny, or the `libm` crate (a new
dependency, so the user's call). It is one decision for the whole workspace and
currently lives in two documents saying opposite things.

### Deferred with reasons, kept on paper (2026-08-27)

**Deliberately not built**, recorded so nobody re-proposes them: hosted tier 3,
the ranked-integrity chain, and `crcbl-mint` as a running service. Their only
consumer was a hosted deployment and the project has none by decision. The
design is kept because it is the expensive half to get right and cheap to keep
on paper. Reversing it is a product call, not a technical one.

**Also decided and worth not re-deriving:** use the Noise Protocol Framework
(`Noise_XX` / `Noise_XXpsk3` / `Noise_IK`) rather than a hand-rolled handshake;
high-entropy generated PSKs only, with a PAKE (SPAKE2/CPace) as the documented
upgrade if community servers want passphrases; 64-bit sequence transmitted
truncated with DTLS 1.3-style implicit reconstruction and an epoch bump on
rekey.

**Honest limit to state alongside any trust claim:** this project never
demonstrates a trust model where the host is adversarial.

### Nothing in this document is built, and three prerequisites are missing too (2026-08-27)

Record; the work this entry still owes is in `docs/backlog.md` under this
heading.

**Decisions already taken, kept so they are not re-argued:** acoustic echo
cancellation is out of scope (research-grade DSP; ship a noise gate and ducking,
recommend headsets, use platform AEC where free). Opus is a sanctioned exception
behind a `VoiceCodec` seam rather than a from-scratch codec. Browser encode
ships **libopus compiled to wasm as the baseline** with WebCodecs used
opportunistically — WebCodecs audio _encode_ support is uneven across Firefox
and Safari, so it is a capability check and never a requirement.

### The grid-inventory kit (2026-08-27)

Decision record; the decision is in `docs/backlog.md`.

**Decided 2026-09-06, built 2026-09-07.** The bind was: topic 34 is written for
breach, and `sample/15-shard.md` was explicit that shard is meant to be the
kit's _second_ consumer ("a kit with one consumer is that consumer's shape
wearing a kit's name"). Breach's inventory sits in milestones that are
native-only by that sample's own reasoning, so nothing had forced the kit and
shard would force it alone — the exact case both plans said to avoid. Shard had
already taken the one deferral available to it (its fight slice shipped with no
item, no currency, no equipped weapon), so its next verb was loot, and loot is
where the kit was forced. Option 1 was taken: build it from shard, breach adopts
it as the second consumer. The record of what that produced is below.

### The kit's model decisions, as built (2026-09-07)

`crates/crcbl-inventory` is the model half of `docs/plan/34-inventory.md`, built
from shard under option 1 above. Five decisions are in the crate header and are
recorded here so they are not re-derived:

- **A footprint is an `8×8` bitmask in one `u64`** (`Shape`, `MAX_SHAPE`).
  Sixty-four bits is exactly the word, so turning a footprint is arithmetic and
  walking its cells is walking its set bits. The cap is deliberately loose — the
  widest container the plan ships is the `5×10` backpack — and a footprint past
  it is refused rather than truncated, because the `x == 8` bit wraps into the
  next row.
- **Four rotations, not two.** The plan calls rotation an involution, and that
  is the rectangle case: a bitmask footprint admits an L, whose quarter turn is
  not its three-quarter turn. A `turned_once` written as a bare transpose — the
  reversal forgotten — is invisible for every container in the shipped table,
  which is why `an_l_turned_four_times_is_the_shape_it_started_as` is written on
  an L and not on the `1×2` pocket item.
- **No `HashMap` anywhere.** Iteration order over one is not something two
  machines agree on, and every answer the crate gives is one a prediction has to
  match. Lookups are linear scans over `Vec`s in file order.
- **The read paths allocate nothing** — `can_place`, `find_slot`, `at`, `slot`,
  `slots`. Occupancy is a `Vec<u16>` sized once at `Grid::new`.
- **`Catalog::to_ron` pins `\n`**, as `crcbl_render::stack::CameraStack`'s
  writer does: `ron::ser::PrettyConfig`'s default is `\r\n` on Windows, and a
  writer whose output depends on the host is not one a data file can be kept in
  git as.

Three shapes departed from the plan's sketch. `Grid::move_within` is one atomic
call rather than remove-then-place: written the other way a refused move deletes
the item and a successful one renumbers its slot, and a drag is holding that
number. `Grid::split` takes the new `StackId` as an argument, because the crate
mints no ids and "re-id it afterwards" is a comment rather than a safeguard. And
a `Grid`'s occupancy map is serialised rather than rebuilt on load, because
rebuilding it needs the footprints, which live in the catalogue, and a
`Deserialize` has none.

### Shard's loot loop, as built (2026-09-07)

The kit's first consumer, and the measurements and decisions it produced.
`apps/shard/src/loot.rs` and `src/panel.rs` are the new modules; the drop is in
`src/game.rs`, the grid is in `src/save.rs`'s payload at version 2.

**What the engine gained: nothing.** That is `sample/15-shard.md`'s exit
criterion, and it held — no line of `crcbl-inventory`, `crcbl-ui` or `crcbl`
changed for this slice. What shard wanted and worked around is in
`docs/backlog.md` as topic 34 findings: a typed drag-drop capability (shard
builds one out of `UiState::interact`'s press capture, inside `panel::draw`, and
would delete it the day part 1 exists), a `PointerUpdate::pixels` to match
`TouchUpdate::pixels` (the engine's own `surface_pixels` is private, so shard
re-derives it), and the `Grid::relink` question below.

**The floor is derived, not saved.** An instance is on the floor exactly when
the foe that left it is down and its stack is not in the character's grid, so
`Stage::restore_floor` recomputes it after a load and the save carries only the
grid. Two copies of one fact is what a save that duplicates an item or loses one
looks like; deriving it is why `a_session_that_looted_comes_back_carrying_it`
can assert the total across floor and grid is the number of felled foes on both
sides of a resume. The sabotage — dropping the "is it already carried" check —
brings the looted stack back on the floor _and_ in the grid.

**The save writes placements and replays them through `Grid::place`** rather
than serialising a `Grid`. That sidesteps the backlog's "a loaded grid's
occupancy map is not re-derived" entirely: the kit paints the map itself and
refuses an overlap, so a payload's grid is one that was actually placeable.
Nothing about that closes the entry — a consumer using serde still needs
`relink` — but it is the shape a consumer can use today.

**Measured: the loot reach is 2.5 m, and 2.0 was wrong.** A living foe's
collider stops the walk, so the character is held about 2.14 m from its centre
by the two capsules' radii, and the cleave reaches 2.2 m
(`foe::STRIKE_REACH_M`). A loot reach below that is one a player cannot use
without stepping onto a corpse first; the first draft used 2.0 and the scripted
pickup test could not reach the body it had just felled. The reach is a distance
only: there is no line of sight on the pickup, so a stack behind a doorpost
within the radius can be taken through the stone — the reach-and-line-of-sight
validation topic 34 asks a server for is not built anywhere.

**The drag does not cross the wire, and the pickup does.** The pickup is one
intent bit (`INTENT_PICKUP`, the sixth of eight in a byte that had three spare,
so `INTENT_BYTES` did not move) applied inside the tick, because what is in
reach and whether it fits are the stage's answers. The drag reaches the stage
through its mutex instead: `Intent` is a flag byte and a bearing, and a cell
pair is neither. Topic 34's `Move` command and its server-side validation are
the kit's server half and are not built; in a single-process loopback the two
sides are the same memory, so this is a seam that is _named_ rather than a rule
that is broken.

**The drop lands where the body falls, not on the foe's post.** A foe that
noticed the character walked at them, so its post is where the fight started. A
resumed session lies the drop on the post, and that is not a second rule: a
felled foe is restored onto its post (`Foe::restore` puts back its health and
nothing else), so "the loot lies with the body" holds either way and it is the
body that moved.

**The drop roll is `lowbias32`.** Which item foe `i` leaves on seed `s` is
`mix(s ^ i.wrapping_mul(0x9E3779B9))` reduced modulo the catalogue's length,
with a second, salted roll for the count — a hash rather than a draw from a
stream, for `apps/sparks/src/show.rs`'s reason: a stream depends on how many
draws came before it, so a zone cleared in a different order would leave
different loot. Measured over 64 seeds: more than eight distinct hauls, and
fewer than 32 of 256 seeds give all three foes the same item.

**The count bound in the save is defence in depth, and this host cannot show it
red.** `decode_grid` refuses a placement count past one per foe before it
computes anything from it. Removing that check turns no test red on a 64-bit
host: the exact-length check catches every large count a file can carry, and the
per-placement checks (a stack id outside the roster, a foe still standing, an id
twice) catch the rest. What the bound owns is the multiply on a **32-bit**
target — `count * PLACEMENT_BYTES` in `usize` wraps for a large `u32` on wasm32,
and a wrapped length that happened to match would index past the slice. It
stays, and this paragraph is why the sabotage log for that slice shows the
length check going red instead.

### Cross-fleet stash: decided and out of scope (2026-08-27)

**Deliberately not built.** Engine stash = per-server-instance store;
cross-fleet stash = backend-project territory, reached through the same
`StorageSource` seam so the engine side never changes. Recorded so the line is
not re-litigated when a fleet is first imagined.

### DECIDED — who forces the grid-inventory kit (2026-08-26)

Decision record; the decision is in `docs/backlog.md`.

Every demo in `docs/plan/sample/` that can run in a browser now does —
`web/build.sh`'s array holds seventeen, and the three that do not are `towers`,
`arena` and `mirrors` — the first two blocked on `apps/editor`, which this file
already records as declined with an override condition. So the next work is
depth against the plans' exit criteria rather than another sample, and `shard`
milestone 1 is the clearest: its criterion is a complete session — explore,
fight, loot, level, save, resume — and loot and level are not built.

**Loot is where it stops being a sample question.** `docs/plan/34-inventory.md`
is the grid-inventory kit, and `15-shard.md` is explicit that shard is meant to
be its **second** consumer: "Topic 34 is written for breach; a kit with one
consumer is that consumer's shape wearing a kit's name." But breach's inventory
sits in its milestone 1 and later, which are native-only by that plan's own
reasoning, so nothing has forced the kit yet and shard would be forcing it alone
— the exact case where a subsystem takes the shape of its single caller.

The options, with the real trade-off:

1. **Build the kit from shard, accept one consumer for now.** Fastest to a
   complete milestone-1 session in a browser, and the browser is where the goal
   is measured. Risks the kit being shard's inventory with a kit's name, which
   is the failure its own plan names.
2. **Give breach a native inventory milestone first, then shard adopts it.** The
   order both plans assume, and the kit gets two consumers before it sets. Costs
   a native-only slice that does not advance the browser goal directly, and
   breach's later milestones are otherwise deliberately parked.
3. **Take shard's slice 2 as fight only, and defer loot.** Needs no new
   subsystem, keeps moving, and leaves this decision until loot actually
   arrives. Milestone 1 stays incomplete either way, so this only defers.

**Option 3 has now been taken and shipped**: `apps/shard/src/foe.rs` is the
fight slice, it needed no new subsystem, and it deliberately added no item, no
currency and no weapon — the character's cleave is a constant in `foe` rather
than something equipped. **So this decision is now the thing in the way.** The
next verb is loot, and loot is where the kit is forced; nothing else in shard's
milestone 1 can be built around it.

### `crcbl-vfx`'s determinism is per machine, not across machines

`tests/determinism.rs` asserts that two runs of one scene leave the pool
bit-for-bit identical, over every array. That holds on one machine. It does
**not** hold across platforms, because `particle::direction` goes through
`f32::sin_cos` for both shapes and libm differs between glibc, Apple and MSVC —
the same limit `docs/backlog.md` already records for the engine's other float
output.

Everything else in the step is exactly-rounded: the hash is integer arithmetic,
the integrator is `+`, `*` and a divide (drag is backward Euler,
`v / (1 + k dt)`, chosen partly for that), and `hash::unit` is a shift and a
multiply by a power of two.

Not a problem for what the plan wants — golden frames are per platform here
anyway, and the destination is a compute shader whose trigonometry agrees with
no CPU's. Worth knowing before anyone puts a particle count in a tick hash.

### P8's ECS access declarations were never reserved, and P2 says they were

Decision record; the decision is in `docs/backlog.md`.

**DECISION NEEDED — which of these P8 does.** They are not the same slice:

1. **Declarations first, no parallelism.** Add the access vocabulary and the
   debug assertions, leave `run` sequential. Closes the gap between the plan and
   the code, and makes the later DAG honest. Lands nothing measurable.
2. **Parallel run first, opt-in per system.** A system that declares itself
   independent runs on the pool; everything else stays on the driver. Delivers a
   measurable win on the systems that are already independent (which, today, is
   all of them) without inventing a vocabulary for coupling that no code
   exercises yet. Risk: "independent" is a promise nothing checks, which is the
   shape of guard this project keeps rejecting elsewhere.
3. **Neither yet — do the broadphase half of P8 first.**
   `crcbl bench --scenario phys` now measures the broadphase, so that half has a
   baseline and the ECS half does not. An ECS schedule with no benchmark behind
   it cannot be shown to have helped.

Recommendation, not taken without the owner: 3, then 1, then 2 — and only after
an ECS benchmark scenario exists to measure against, for the reason the
profiling plan gives about numbers that cannot be compared.

### DECIDED — a refit-only tree degrades, and the number is now known

Decision record; the decision is in `docs/backlog.md`.

`Bvh::update_aabb` never re-picks a leaf's place: it writes the new box into the
leaf and grows the ancestors it walks back through. Its own doc says callers
whose elements travel should remove and re-insert instead, and nothing in the
engine does. `crcbl bench --scenario phys --ticks N` measures what that costs.

**Measured 2026-08-23**, release, 2000 bodies of radius 0.5 in a 48-unit arena,
reproduced twice on this machine:

|  ticks | depth | nodes | builds | query p50 | neighbours/query | ns per result |
| -----: | ----: | ----: | -----: | --------: | ---------------: | ------------: |
|      1 |    12 |  3999 |      1 |    516 µs |             5.96 |          43.3 |
|    100 |    12 |  3999 |      1 |    559 µs |             5.87 |          47.6 |
|   1000 |    12 |  3999 |      1 |    735 µs |             5.71 |          64.4 |
|  10000 |    12 |  3999 |      1 |   1641 µs |             5.23 |         156.8 |
| 100000 |    12 |  3999 |      1 |   5299 µs |             3.98 |         666.2 |

Read the last column, not the fourth: the crowd thins as it walks, so raw query
time understates the decay. **The tree's reported shape never changes** — depth
12, 3999 nodes, one build — at every tick count, which is the finding rather
than a null result. The boxes stay exact for the current positions while the
topology remains the answer `Bvh::build` gave for where the crowd was _placed_.
Refit and build cost stay flat throughout, so a running game sees none of this
in its physics tick and all of it in its broadphase queries.

It is inside run-to-run noise below 100 ticks and unmistakable by 1000 — about
17 seconds of simulation at 60 Hz, an RMS displacement of roughly one world unit
against a 0.5-unit body radius.

**The decision, with the trade-off now priced.** A rebuild costs about 490 µs on
this fixture and buys back up to 4.8 ms of query time per tick at the far end.
The options are not equivalent:

1. **A rebuild cadence** — rebuild every N ticks, or when a cheap staleness
   estimate crosses a threshold. Simple, and it makes one tick in N expensive,
   which is a frame-time spike a game has to absorb.
2. **Remove-and-reinsert on the move**, which is what `update_aabb`'s own doc
   recommends. Spreads the cost evenly and keeps the tree's placement honest;
   costs more per update than a refit, on every update, including the many that
   barely move.
3. **Refit with a fattened box**, the usual answer in the literature: a body
   moves inside its own slack without touching the tree, and only reinserts when
   it leaves. Bounds the degradation and costs one comparison per update, at the
   price of a looser tree from the start and a slack constant to choose.
4. **Leave it**, and say so in the API: below a few hundred ticks the decay is
   not measurable, and a game that rebuilds on level load may never reach the
   range where it matters.

**Not verified:** any target but Linux x86-64, and no tick count above 100000 —
at 2000 bodies that is already 200 million refits per iteration, and the trend
had not plateaued.

**Considered and declined for the fixture:** reflecting bodies off the arena
walls, which would hold the density constant and make the raw query timing
comparable across tick counts without the per-result normalisation. It cannot be
done without moving bodies that start within one step of an edge, which would
change what `--ticks 1` reports and cost the scenario its bit-identity with
every number recorded before the flag existed.

### `System<T>`'s internals are not observable, so a desync surfaces as a panic

`crates/crcbl-ecs/tests/churn_soak.rs` catches a component row outliving its
entity, but only through what the public API exposes — the row count, `get` on a
live entity, `get` on a stale handle, and the dense-versus-live pairing. The
lengths of `index_to_entity` and the size of `entity_to_index` are not
reachable.

Measured while red-checking the soak: breaking `System::detach`'s moved-entity
fix-up, or its `index_to_entity.pop()`, does not fail an assertion. It panics
inside `System::get` with
`index out of bounds: the len is 4 but the index is 4`. The defect is caught,
which is what matters, but by a crash in the code under test rather than by a
test saying what is wrong.

A direct observable needs a `#[cfg(test)]` accessor on `System<T>`. That was
deliberately not added — widening the API for a test is the thing to avoid, and
the indirect coverage does hold. Worth doing only if this bites someone.

**Also measured, and the reason one assertion is not redundant:** the stale-key
leak — `entity_to_index.get().copied()` where `remove()` belongs — is caught
**only** by the end-of-run loop that re-queries every despawned handle. Every
per-tick assertion stays green through it. Do not delete that loop as duplicated
work.

## What the deleted 36-contact-solver plan left behind (2026-09-24)

Record; the built part of the plan is rungs 0 to 5 of its ladder in
`crcbl-phys`: rotation and materials (`mass.rs`, `material.rs`,
`crcbl_core::trig`), the split broadphase (`contact/broadphase.rs`), analytic
and box-box manifolds (`contact/manifold.rs`, `contact/manifold/box_box.rs`),
the Soft Step solver (`contact/solver.rs`), islands and sleep
(`contact/island.rs`), sweeps (`contact/sweep.rs`), the static triangle mesh
(`mesh.rs`, `contact/manifold/triangle.rs`), five joint kinds with breaking and
per-group substeps (`joint.rs`, `contact/joint/`, `contact/group.rs`), and
beside the ladder compound bodies (`compound_shape.rs`), query layers and
`PhysicsSystem::put_to_sleep`; `apps/tumble` has a room for each rung. What it
left open is in `docs/backlog.md` under _Contact solver L2/L3: rungs 0 to 5
built, and what they left_, _Contact solver rung 6: colouring, the wide kernel
and parallel stages_, _`crcbl phys stack --check` and the solver's profiler
rows_, _Physics debug suite: draw, scrub, query visualiser_ and _Buoyancy and
wind force providers_ (rung 7). It specified topic 5's L2 (contacts) and L3
(constraints), which ragdolls, grenades, dropped loot and vehicles need.

Code cites the plan as "contact-solver rung 3", "contact-solver decision 4" or
by a section's name. Those resolve here:

| Citation              | What it specified                                                                                                                              |
| --------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Rung 0, Spin          | Inertia tensors, quaternion integration and the gyroscopic step; dense body sets; friction and restitution from materials; pinned trigonometry |
| Rung 1, Pachinko      | Analytic sphere and capsule manifolds; split trees, move buffer, pair set; the Soft Step with warm start, speculative contacts, restitution    |
| Rung 2, Tower         | Boxes and hulls: cached SAT, clipping, four-point reduction, feature ids; GJK for round shapes against hulls; centroid and twist friction      |
| Rung 3, Settle        | Persistent islands, lazy splitting, island sleep and the wake rules                                                                            |
| Rung 4, Bullets       | Fast-body sweeps against statics, the bullet flag, dropped time                                                                                |
| Rung 5, Bridge        | Joints, limits, motors, breaking and extra substeps per group; the static triangle mesh with active edges                                      |
| Rung 6, Pit (unbuilt) | Persistent colouring with an overflow colour, the wide kernel and its scalar twin, staged `crcbl-jobs` execution, contact recycling            |
| Rung 7, Pool and gale | Buoyancy and wind force providers (unbuilt)                                                                                                    |
| Decision 1            | The solver: Soft Step, and its parameters                                                                                                      |
| Decision 2            | The narrow phase: analytic pairs, SAT for boxes and hulls, no EPA                                                                              |
| Decision 3            | The broadphase: split, fattened SAH trees                                                                                                      |
| Decision 4            | Islands for sleep, colours for solving; the wake rules                                                                                         |
| Decision 5            | Continuous collision: speculative contacts, then single-pass sweeps                                                                            |
| Decision 6            | Joints: impulse joints in the same solver                                                                                                      |
| Decision 7            | Precision: `f64` positions and an `f32` solver interior                                                                                        |
| Decision 8            | Data layout: dense and generational, not hash maps                                                                                             |
| Decision 9            | Tests grow to the benchmarks' size                                                                                                             |
| Materials, `combine`  | Friction and restitution on the collider property block that acoustics, nav and ballistics read, with a per-property combine rule              |
| L3 joints             | Fixed, hinge, swing-twist cone, slider, distance and a 6-DOF joint with per-axis lock, limit and motor; breakable by force                     |
| "Before the stairs"   | Rung 5's static triangle mesh, which tumble's ragdoll stairs need                                                                              |
| Debug + tooling       | Debug draw of contacts, ids, islands, sleep and joint frames; profiler rows; `crcbl phys stack --check`                                        |

**The finding the decisions rest on (engine research, 2026-09-15).** The user
delegated the solver family to research after deciding physics stays built from
scratch, with a physics showcase (`apps/tumble`) driving it. Box2D v3, Box3D,
Rapier 0.35 and Avian had converged on one design: _islands exist for sleeping
only, and the awake set is solved as one pool split into persistent constraint
colours_ whose constraints never share a body. Jolt, which rebuilds and sorts
islands each step, was the runner-up. The decisions superseded the plan's older
sections wherever they disagreed.

**Decision 1 — the solver is the Soft Step, for contacts and joints.** Collision
runs once a tick; the solver runs 4 substeps of 1 biased and 1 relax iteration,
warm-started, with no convergence loop. Contacts are soft at 30 Hz with damping
ratio 10 (static contacts at twice that), push-out capped at 3 m/s; joints at 60
Hz with damping 2; speed capped at 400 m/s and rotation at π/4 a substep.
**Restitution is its own pass after the substeps**, above 1 m/s, because
speculative contacts damp bounces. **A long chain or tall stack gets more
substeps for its group, not more iterations.** Evidence: Catto's Solver2D
comparison (Soft Step needs 4 passes for what PGS reaches in 8), Macklin's
_Small Steps_, and adoption by Box2D, Box3D, Rapier, Avian and PhysX's TGS.

**Decision 2 — analytic pairs and SAT, with no EPA.** Sphere, capsule and their
pairs are analytic; boxes are hulls with a separating-axis test cached per pair,
face clipping, reduction to at most four points, and **flip-invariant feature
ids** for warm starting. Friction acts at the manifold's centroid with a twist
term. Sphere or capsule against a general hull is to use GJK on the core shape
with a SAT fallback when deep.

**Decision 3 — split, fattened trees.** A static tree and a dynamic tree, the
static one rebuilt after load; fat margins of min(5 cm, ⅛ of the extent); a move
buffer so only enlarged proxies query; a pair set; persistent contacts created
before they touch.

**Decision 4 — islands for sleep, colours for solving.** Islands are persistent,
merged on a contact beginning and split lazily, the sleepiest one a tick, and
sleep whole after 0.5 s below 0.05 m/s. Awake constraints are to be coloured
greedily with per-colour body bitsets, persistently, with an overflow colour.
**Order comes from persistent arrays, never a per-step sort**, and parallel
narrow-phase results merge through per-worker bitsets, so `crcbl-jobs` schedules
work without deciding its order and the single-threaded Pages build hashes the
same as N threads. A body wakes on a contact beginning with an awake body, an
applied impulse, a new joint, or **a touching neighbour's removal** (Jolt does
not wake on removal, a trap worth a test); **a query wakes nothing**.

**Decision 5 — speculative contacts for everything, then sweeps for fast
bodies.** After the solve, a body that moved more than half its smallest extent
sweeps against statics (a bullet flag adds dynamic and kinematic bodies), and
lost time is dropped rather than re-solved: single-pass, as Box2D, Jolt and
PhysX do. The 10 km/s projectile stays the ballistics topic's segment test. This
inverted topic 5's "sweep to the time of impact, then solve".

**Decision 6 — impulse joints in the same solver**: revolute, spherical with
cone and twist limits, distance and rope, weld, prismatic; limits, motors,
breakable by impulse; capsule ragdolls are the ragdolls topic's server ragdoll.

**Decision 7 — `f64` positions, an `f32` solver interior (decided by the user,
2026-09-17).** Positions stay `f64` in sector-local space, since `f32` resolves
only 0.125 m at a 2²⁰ m sector's edge. The interior (velocities, deltas,
impulses, effective masses) can be `f32` because the solver already works on
deltas and anchors relative to each body; the reason to switch, at rung 6, is
width — wasm SIMD has two `f64` lanes against four `f32`, and Jolt measured a
naive all-double build at over 2× slower against 5–10% for its boundary design.
`f32` is exactly as deterministic as `f64`; only the hash differs, so every
target uses one precision. This amends topic 5's locked "f64" line. The
simulation's `sin` and `cos` are built in-engine in `f64` on
`crcbl_shaders::trig`'s pattern, which is what rung 0's "pinned trigonometry"
means.

**Decision 8 — dense and generational storage, after Box3D's.** Generational
body ids mapped to (set, index), the entity map only at the ECS boundary; a cold
body record and a hot body state; contacts in a persistent pooled array with
per-body edge lists and a colour index; constraints prepared each tick into
per-colour struct-of-arrays blocks whose scalar path does the same per-lane
arithmetic, so scalar and SIMD builds hash the same. **Hashing canonicalises
−0.0 and NaN; no `mul_add`, no relaxed SIMD.**

**Decision 9 — tests at the benchmarks' size.** The regression pyramid's base is
20 in CI and 100 (5050 boxes) as the benchmark, as Box2D's and Box3D's large
pyramid; energy never rises; penetration stays under the slop; the hash is equal
across thread counts, SIMD and scalar, native and wasm. For pricing, Box3D
reports that pyramid at about 10.4 ms a step on one SSE2 thread, 25 ms scalar
and 1.7 ms on eight threads (Ryzen 7950X, 60 Hz, 4 substeps).

**Rules that came from the older sections and still bind.**

- **Warm starting from persistent contact ids is non-negotiable**: it is what
  makes a stack of crates stand still instead of shivering.
- **One material asset per surface, four consumers**: friction and restitution
  live on the same collider property block acoustics, nav and ballistics read,
  with a per-property combine rule — `SurfaceMaterial` and `CombineRule`.
- **Contact impulses above a threshold raise `KineticContact`**: damage is a
  by-product of solving, not a second collision system.
- **The character controller stays kinematic.** It queries and sweeps and is not
  solver-driven; dynamic bodies react through one-way pushes with a force
  budget, so the player shoves crates and a crate cannot launch the player. That
  reads as a decision, not a bug.
- **Scope:** soft bodies, cloth, fracture and fluids are not the contact
  solver's; anything deformable is a separate topic with its own case. Vehicles
  are joints plus wheels-as-raycasts, post-MVP.
- **The stability suites define "good enough" numerically**, because solver
  quality is a tuning surface with no natural end, and substeps buy what
  iteration counts cannot.

**Measured departures, by rung.** The module docs carry the detail; these are
the ones that change what a reader would assume.

- **Rung 1: the speculative distance grows with the pair's closing speed**,
  because decision 5's fixed four slops let a 30 m/s ball through a 2 cm plate;
  **separation within a tick is tracked to first order**, since Box2D's turned
  anchors made a rolling ball slip.
- **Rung 2: sphere and capsule against a box stay analytic** — the closest point
  is a clamp, exact and cheaper — and GJK waits for the hull it is for. **A
  column does not stand at 30 Hz past Greenhill's height**: a soft contact's
  stiffness is `m ω²` whatever it carries, so it buckles past
  `(1.96 ω² w / g)^⅓` cubes of half-extent `w` (fifteen one-metre cubes at 30
  Hz; measured, fourteen stood and seventeen fell). Measured: a base-20 pyramid
  held ten seconds, top box sunk 2.76 cm, 99.7% of ids persisted; 766 µs a tick
  in the solver and 186 µs in the narrow phase, release build, Ryzen 9 9950X3D,
  one thread, scalar `f64`.
- **Rung 3: turning is judged by angular speed**, not the farthest point's
  speed; **a kinematic body wakes what it touches only while it moves**;
  kinematic and static bodies join no island; **a static placed on a sleeper
  wakes it**, a rule decision 4 does not list; **contacts stay in one pool**, a
  sleeping island's skipped on a look at their records (96 µs a tick for the pit
  at rest); **a stack sleeps before it is still** (2.4 mm aside, where awake it
  creeps back to 0.39 mm). Measured: a base-20 pyramid asleep at tick 59, 850 µs
  a tick awake against 1.6 µs asleep.
- **Rung 4: a path stops if it gets a linear slop into a shape, or deeper than
  it began, and is put a slop short** — Box2D's "a slop short" test undid the
  solve's landings (2046 bodies stopped and 10.9 s dropped in twenty seconds,
  against 229 and 1.5 s). There is no circle at the centroid; turning is
  measured at the cores; the path is interpolated, not the substeps'. Time of
  impact is conservative advancement (Mirtich 1996), the turning bounded by
  `4 tan(α/2)` for a turn of `2α`. **One-point contacts twist against a Hertz
  patch** of radius `√(R δ)`, clamped between `R` and half a millimetre, because
  a ball otherwise spun on a floor for ever.
- **Rung 5, meshes: each triangle is a broadphase proxy of its own** (as each
  compound part is), contacts are one-sided (Box2D v3's chain-segment rule) and
  queries two-sided, **active edges are Jolt's** (`ActiveEdges.h`), without its
  movement hint, and there is no contact reduction across triangles. A
  degenerate triangle is refused, not skipped, since skipping renumbers the
  rest. A mesh goes on static and kinematic bodies only.
- **Rung 5, joints: each kind transcribes Box3D's solver of the same name**
  (commit `9e5a4cde`). **A joint's angular impulses turn the body in full**,
  where contacts keep rung 0's midpoint rule (the midpoint lost half of every
  joint correction and a 21-plank bridge gained 3.4 kJ; the full rule leaned the
  14-cube column 4.9 cm). **A group's constraints stiffen in proportion to its
  substeps**, capped at a quarter of the substep rate. **A broken joint is taken
  out**, where Box3D only reports it. **Joint angles use `b3Atan2`**, since the
  crate calls no platform transcendental.
- **Compounds: each part is a broadphase proxy of its own**, so every contact is
  a part pair's and ids, the SAT cache and warm starting apply unchanged; mass
  sums the parts at one density, counting an overlap once per part.

**Considered and declined:**

- **XPBD** — Avian left it: deep overlap was explosive, it never truly settled,
  friction was weaker and collision ran every substep, and Avian was 4–6× faster
  after switching; Catto reports friction and far-from-origin precision
  failures.
- **PGS with a position solve** (Jolt's) — works, but needs 10 + 2 iterations.
- **GJK/EPA with convex margins** (Jolt, PhysX's PCM) — visible gaps, and
  warm-start points matched by distance rather than by feature.
- **Reduced-coordinate multibodies** — tree-only (a bridge needs loop-closing
  constraints anyway), slow to add and remove, no joint forces for breaking, and
  a second solver.
- **Islands solved independently, ordered by lowest entity id with contacts
  sorted per island** — the plan's original determinism scheme, replaced by
  decision 4's persistent colours.
- **Waking on a query** — a query touching a sleeper wakes nothing.
- **Sweep to the time of impact, then solve** (the plan's and topic 5's original
  continuous-collision scheme) — replaced by decision 5.
- **One broadphase proxy per compound body**, with the narrow phase walking part
  pairs — it would repeat the broadphase's cull every tick and give a contact
  several normals.
- **A whole-system `ContactSettings::TALL_STACK`** — removed before it shipped
  in favour of per-group substeps.

## Steamworks: four decisions, all taken (2026-08-22, settled 2026-09-23)

Decision record, as the options stood on 2026-08-22; the answers are in
`docs/backlog.md` under this heading, and the rules that came of them in
`docs/notes/backends.md` under _What the deleted 42-steam plan left behind_.

- **The binding route, and whether this MIT repo publishes our own flat-API
  declarations.** (a) hand-write `extern "C"` declarations and `repr(C)` structs
  transcribing no header text — zero dependencies, a loader we control, a drift
  gate against the SDK's `steam_api.json` as the net. (b) depend on
  `steamworks-rs` — the ABI risk becomes theirs, at the cost of a link-time
  `DT_NEEDED` (so no graceful "Steam absent"), a vendored SDK in the dependency
  graph, and an event model to wrap over. The plan recommends (a). Either way a
  new dependency is the user's call by standing rule. Note the access agreement
  licenses the SDK "solely to develop" and does not address publishing one's own
  ABI declarations; the ecosystem precedent is strong (Steamworks.NET, a decade,
  MIT) but **precedent is not permission, and this was read by an agent and not
  a lawyer**.

- **Cloud saves: Auto-Cloud config, or a `StorageSource` backend.** (a) zero
  code over `crcbl-store`'s existing atomic layout, sync timing is Valve's and
  no in-game sync UI is possible. (b) a `SteamCloudStorage` backend over
  `ISteamRemoteStorage` — per-file control, quota, conflict surfacing, and a
  real slice of async work behind the seam. The plan leans (a) first.
- **Steam Input, or topic 19's own backends alone.** Declined in the plan
  because `ISteamInput` is a competing action mapper against `ActionMap`. The
  counterweight is Steam Deck verification, which requires first-class Steam
  Input. If Deck becomes a target it returns as a slice feeding `ActionMap`
  through `Binding::Virtual`, origins and glyphs included.

## Owed

Records lifted out of the `## Owed` list in docs/backlog.md, which keeps the
work.

- **The listener standoff moved from the emitters onto the listener, and the
  subtraction changed precision with it.** Every sample used to compute
  `compute_cue([0,0,0], [dx, dy, 1.0])` — the listener at the origin, and "one
  unit in front" added to _each emitter's_ Z with the same comment copied into
  three files. That standoff is a fact about the camera, so it now sits on the
  listener (`LISTENER_STANDOFF` in each sample, listener at `z = -1`, emitters
  at their true Z).

  `emitter − listener` is arithmetically the same, but **not bit-identical**,
  and the agent's report claiming it was is wrong. The samples that subtracted
  first — horde and flappy — did `(at.x - listener.x) as f64→f32`, one rounding;
  the new path casts each coordinate to `f32` and subtracts inside
  `compute_cue`, two roundings. The error is bounded by the coordinate
  magnitude, and horde's arena is `ARENA_HALF_WIDTH` 48 by `ARENA_HALF_HEIGHT`
  36, so it is on the order of 1e-5 on a direction that gets normalised — far
  below audibility and below every assertion in the suite, which is why nothing
  moved. Recorded because "bit identical" is what someone would otherwise assume
  when reading the diff, and it would be the wrong thing to rely on if these
  coordinates ever grow.

- **Resolved by the review's deletion: the `play_panned` panics.** The review
  named an `id as usize - 1` underflow and a `fade_env` underflow in
  `apps/breakout/src/audio.rs`. Neither can happen: the first was fixed before
  the review was even filed — the code takes `bank.create_voice(id)` behind a
  `let Some(…)` guard and says so — and `play_panned` itself no longer exists,
  every sample having moved onto `crcbl_audio::mixer`. The `fade_env` half was
  never re-checked and now has no function to check.

## P5B — the job system, and the two decisions in front of it

Record; the owed work is in docs/backlog.md under the same heading.

- **Every read-only query has the shared form, and one caller runs it in
  parallel.** `OverlapQueries` and `EntityOverlapQueries` carry
  `overlap_sphere_into`, `overlap_aabb_into`, `cast_ray` and `sweep_sphere`
  (plus `sweep_sphere_excluding` at the collider layer), each `&self` with a
  caller-owned `QueryScratch`, and each sharing one `*_core` with the
  `&mut self` form so the two cannot drift. `apps/horde`'s steering spends the
  exclusive borrow once for a view and then calls
  `EntityOverlapQueries::overlap_sphere_into` from inside a
  `pool.par_for(.., STEER_CHUNK, ..)` with a thread-local `QueryScratch` — held
  by `steering_is_bit_identical_however_many_workers_run_it` in
  `apps/horde/src/game.rs` and by
  `the_view_names_the_right_entities_from_every_thread_at_once` in
  `crcbl-phys`'s `system` module. It is the only adopter.

  Two things measured while doing it, both worth knowing: `cast_ray`,
  `overlap_aabb` and `sweep_sphere_excluding` **stopped allocating per call** as
  a side effect of routing through the view (`cast_ray` used to build a 64-slot
  stack and a hit vector on every cast); and `QueryScratch` is now four `Vec`s
  per thread, which is what a `par_for` adoption sizing per-chunk state is
  paying for.

**The atomics are checked by Miri and by nothing else.** x86-64 is
total-store-order, so a `Release` store and a `Relaxed` one compile to the same
instruction and weakening one is invisible to any test on this machine — which
is why the Miri job is load-bearing. **It runs per commit**, as `ci.yml`'s
`miri-jobs` job — moved there 2026-08-23 on the argument that the per-commit
value is concentrated in this one small crate, while the full crate list is
minutes of interpretation per PR and stays weekly in `cron.yml`. So an ordering
regression is caught on the push that introduces it, and the pre-push ritual and
manual cron trigger this entry used to prescribe are no longer needed. The
weekly job went red on 2026-08-03 for want of a `libasound2-dev` install and was
found only because it was briefly on the per-PR path, which is the argument that
moved it.

**The primitives DO run on a weakly-ordered machine, and this entry said they
did not.** Corrected 2026-08-23: the entry asked for an aarch64 runner as
independent evidence beside Miri's model, called it unattempted and priced it at
a second `test` leg. That leg exists — `build + test (macos-latest)` in `ci.yml`
runs the workspace under nextest on **`aarch64-apple-darwin`**, and all forty of
`crcbl-jobs`' tests pass there on every run (counted off `14c89de`'s CI log).
Nobody needs to build it.

This entry then claimed the two concurrent tests were not written to be
adversarial and that making them so was the slice. **That was wrong too**, read
2026-08-23: `a_concurrent_producer_and_consumer_move_every_item_in_order`
(`ring.rs`) moves 20 000 items across a real thread boundary and asserts each
arrives in order, and `a_concurrent_reader_never_sees_half_of_a_state`
(`mailbox.rs`) publishes 20 000 two-word states and asserts no read is torn and
none goes backwards. Both are already the shape that catches a reordering, both
are bounded so a wedged primitive fails red instead of hanging, and both run on
aarch64 every CI run. There is no test-writing slice here.

**Measured 2026-08-23: they do not fail for this reason on that machine, so the
aarch64 leg is reassurance and not evidence.** The red-check behind these
orderings had only ever been run under Miri, where the model reports the race;
on aarch64 the hardware has to be caught in the act. So it was run there. A
throwaway branch weakened both orderings at once — `ring.rs`'s `tail` store from
`Release` to `Relaxed`, `mailbox.rs`'s handoff swap from `AcqRel` to `Relaxed` —
and CI was dispatched against it. **The entire run went green**, and the two
tests demonstrably executed rather than being skipped:
`build + test (macos-latest)` logs `PASS` for both, at 0.011 s and 0.231 s.

Read it for exactly what it is. One run of 20 000 iterations on Apple silicon
produced no violation, which says these two tests are not sensitive enough to
catch a reordering, not that a reordering could never surface. It also weakens
the ordering for the compiler as well as the hardware, so a green run does not
even isolate which of the two would have had to misbehave. The practical
consequence: **Miri remains the only check that actually holds these
orderings**, and an aarch64 job should not be cited as covering them. If that
coverage is wanted for real, the instrument is a targeted stress harness — many
short runs with interleaving pressure and a failure counter — not the existing
tests and not another runner.

- **A mode comparison cannot catch a defect that is symmetric across modes**,
  and this was measured rather than assumed: dropping the last chunk of every
  `par_for` leaves both worker-count tests green, because a pool with no workers
  drops it too. Eight other horde tests go red on that mutation, which is what
  actually covers it — worth knowing before anyone reaches for the worker-count
  tests as a general correctness net.
- **One deque, not one per worker.** Only the driving thread pushes today, so a
  per-worker deque would be a queue nothing ever puts anything in. What needs
  them is `scope(|s| …)` fork-join (the design lists it for BVH build), where a
  running chunk spawns more work. Not written, and nothing calls it.
- **`Mutex` + `Condvar` for the sleep, in the frame path.** The design's rule is
  no mutexes in the frame path; this takes one per _submission_, not per job,
  and a worker takes it only on its way to sleep. A futex-style parking scheme
  would remove the lock, and needs the profiler to say whether it is worth the
  reasoning.
- **Considered and declined: aborting the remaining chunks when one panics.**
  Running them all instead lets the panic be reported by chunk index — the
  lowest wins — so a panicking `par_for` fails identically with and without
  threads.
- **A broken completion count hangs the suite rather than failing it.** Three
  mutations each wedge `par_for`'s wait loop instead of going red, because a
  chunk that never finishes is exactly what that loop waits for. A deadline in
  the wait loop would fix the symptom by putting a timeout in the frame path,
  which is a worse trade; the honest note is that this class of defect looks
  like a hang.
- **Considered and declined: `crossbeam-deque`.** It would be the right one, but
  neither it nor any `crossbeam-*` nor `rayon` is in `Cargo.lock`, and a new
  dependency is the user's call. Worth revisiting if one arrives for another
  reason: its growable deque would remove this one's capacity ceiling, past
  which `par_for` runs the extra chunks on the driver.

**`ring` does not implement drop-oldest**, though `21-jobs.md` lists it beside
drop-newest as an overflow policy. It cannot be done from the producer: the read
cursor belongs to the consumer, and a producer advancing it to make room would
be a second writer to it, which is exactly what makes an SPSC ring cheap. `push`
hands the item back and counts the refusal instead, leaving the policy to the
caller. If a real consumer turns up wanting drop-oldest, the honest options are
a consumer-side drain-and-discard or an MPSC design, not a flag on this one.

- **The Web Worker spawn backend is in, and `default_spawner` yields it.**
  `crcbl_jobs::workers` (`Workers`, plus the `shim` module's
  `__crcbl_web_jobs_*` exports) is P5B step 3 of `21-jobs.md`'s order.
  `web/build.sh --threads` builds it and `web/tools/worker-gate.mjs` runs real
  `node:worker_threads` workers through it. What is left is below; the shape of
  the bootstrap is no longer an open question and the paragraphs arguing it have
  been deleted rather than annotated.

- **`Pool` CAN be driven from the browser's main thread, measured — which
  contradicts what this entry used to predict, and the prediction's reasoning is
  still sound.** The argument was: `Pool::par_for` takes `Shared::sleep` (a
  `std::sync::Mutex`) on every submission and `pool::work` waits on `Condvar`
  before parking; on `wasm + atomics` std's `Mutex::lock` reaches `futex_wait`
  (`sys/sync/mutex/futex.rs`, then `sys/pal/wasm/atomics/futex.rs`), which is
  `memory_atomic_wait32`, which a browser main thread **throws** on rather than
  blocking in.

  **What actually happens, 2026-08-23, Chromium 151, `web/run-jobs-e2e.sh`
  against `web_worker_gate.wasm`:** 3000 `par_for` calls driven from the page's
  main thread with eight workers up and parking between calls, no trap, every
  checksum right. The reconciliation is that `Mutex::lock`'s fast path is one
  `compare_exchange` and only a **losing** one reaches `futex_wait`, so the trap
  needs the driver to lose a race for `sleep` against a worker on its way to
  park. That window is real and this workload does not hit it. **So this is "not
  observed", not "cannot happen"** — a heavier or differently shaped consumer
  could still find it, and a trap is fatal to the frame rather than slow. The
  topology `21-jobs.md` settled (the game worker owns the pool, main forwards
  and presents) is still the arrangement to prefer; it is no longer the only one
  known to run.

  `crcbl_jobs::workers`'s own queue never had the question: it is taken with
  `Mutex::try_lock` in a spin (`workers::hold`), never `lock`, precisely so the
  drain half is legal on main.

- **A worker that skips `__wasm_init_tls` does not necessarily trap, which
  corrects the entry that used to sit here.** Measured against
  `crates/crcbl-jobs/examples/web_worker_gate.rs`: with the call omitted,
  `__tls_base` is simply left at zero, every worker's thread-locals alias one
  address near the start of linear memory, and a `thread_local!` with a `const`
  initialiser reads and writes it without complaint. The earlier
  `RuntimeError: unreachable` came from a different crate, but **not** because
  its thread-local was lazily initialised — it was `const { Cell::new(0) }`, the
  same shape. The cause was measured on 2026-08-23 by reading `__tls_base` in a
  fresh worker instance before any init: in that build it starts at **1048576**,
  which is also the initial `__stack_pointer`, so a worker skipping the call
  wrote its thread-locals into the top of the static stack region and the
  corruption trapped. In the gate's build it starts at **zero** and the aliasing
  is harmless. **The initial value is a layout accident of the module**, so both
  outcomes are real and neither is a rule: a trap here is luck, and a gate must
  observe TLS separation directly. **It defeated this gate's first shape**: a
  "count each thread once" flag was satisfied by the first worker setting the
  shared flag for all of them, and the red check came back green. The observable
  that works is `gate_tls_shared` — a thread-local holding **the caller's own
  frame address**, so a thread that finds an address its stack could not have
  produced is reading someone else's TLS. Anyone writing another TLS assertion
  should start there. **Confirmed again in Chromium**, 2026-08-23: with
  `?no-init-tls` the browser gate reports no trap, no clobbered stack and a
  green checksum, and `gate_tls_shared` is the one assertion that goes red. Five
  runs, the same result each time.

- **The handle in the spawn ABI is a table index, not a pointer, and that was a
  deliberate departure.** The design sketched here previously said to double-box
  `Work` to a thin pointer and hand JS the address. It is not needed:
  `workers::Queue::handed_out` keeps the `Work` owned by Rust and gives JS an
  integer, so `__crcbl_web_jobs_entry` validates by lookup and an invented,
  replayed or corrupted handle finds nothing instead of being read as an
  address. The consequence worth knowing is that **the backend adds no `unsafe`
  to `crcbl-jobs` at all** — the crate's unsafe is still only `mailbox`, `ring`
  and `pool`. Cost: lookup is a linear scan of the in-flight list, which is
  worker count and not work count, and handles are unique only up to `u32::MAX`
  — `next_handle` saturates rather than wrapping, so `0` can never be minted,
  but past the ceiling every later request carries `u32::MAX`.
  `__crcbl_web_jobs_entry` `swap_remove`s one matching entry per call, so each
  work still runs exactly once even then; what a collision costs is the pairing
  between a worker's name and its work. Four billion spawns away, and stated
  because "never reused" is not what the code does.

- **`--max-memory` is passed and only reachable as the maximum the gate reads
  back out of the import.** Unchanged from when `--threads` landed.

- **`+mutable-globals` is asserted and cannot be made to fail.** Dropping it
  from `-C target-feature` changes nothing — the artifact still exports a
  `__stack_pointer` JS can write. Forcing the opposite, `-mutable-globals`,
  makes `rust-lld` refuse to link:
  `mutable global exported but 'mutable-globals' feature not present in inputs: __stack_pointer`.
  With `--no-check-features` added to suppress that, the global is still mutable
  and still writable. The check's _mechanism_ has teeth — writing an immutable
  global throws `TypeError: Can't set the value of an immutable global`,
  verified against `__tls_size` — but on this toolchain no flag combination was
  found that makes `__stack_pointer` fail it. Belt and braces, not a guard.

- **`--shared-memory` cannot be dropped on its own.** Without it `rust-lld` does
  not synthesise `__wasm_init_tls`, `__tls_size` or `__tls_align` at all, so the
  link fails on the `--export` of them before an artifact exists. The isolated
  red check meant dropping those three exports as well, and then the gate
  reports the unshared memory by name. The flags are not independent.

- **`jobs-worker-e2e` is measured, on one runner and one commit.** The first
  run, with a cold `Swatinem/rust-cache`, took 5.0 min end to end on
  `ubuntu-latest`: 48 s for the jobs gate, 223 s for horde's, the rest toolchain
  and browser setup. Locally with warm target directories the same two are 22 s
  and 151 s. One sample of each, so the timeout is headroom rather than a
  budget.

### Deferred: browser multiplayer over WebRTC

**The only route that survives the no-infrastructure constraint**, and it is
recorded rather than refused so the decision is reopenable.

Data channels with **manually exchanged connection codes**: peer A creates the
connection, waits for ICE gathering to complete so candidates are embedded in
the SDP, and the compressed base64 of that is a "code" pasted to peer B, who
answers with one of their own. No signalling server. It maps onto topic 23's
channel semantics **better than WebSocket would have** — DataChannel offers both
ordered-reliable and unordered-unreliable, so the unreliable channel survives.

Against it: a third transport to maintain; a JS shim owning `RTCPeerConnection`
(the same `extern "C"` shape `crcbl-audio`'s web module already uses, so no
`wasm-bindgen` in any crate); a code that is hundreds of characters rather than
a room code; two round trips of copy-paste with both players in live contact
elsewhere; two peers realistically, since full mesh is N(N−1) exchanges; STUN
needed off-LAN and TURN needed behind symmetric NAT, which is the part that
costs money. Free public STUN is plentiful and the Open Relay Project offers a
free TURN tier.

**Do not fold it into bracket** — manual pairing is the antithesis of
matchmaking. If it is ever built it wants its own small sample whose subject is
the transport seam over a third transport shape.

## Narrow matchmaking stretches an Elo ladder (2026-08-24)

Decision record; the decision is in docs/backlog.md.

Found while building `apps/bracket`, and it needs a decision before the sample
can claim a rating system that converges.

**What happens.** `bracket`'s population converges and then comes apart. Mean
distance between a player's rating and their true skill, 64 players, five seeds,
tight agreement across all of them:

| ticks  | mean error | ladder spread (true range is 1000) |
| ------ | ---------- | ---------------------------------- |
| 2 000  | 54–58      | 978                                |
| 10 000 | 132–139    | 1 860                              |
| 30 000 | 325–335    | 2 689                              |

The mean rating stays put (1499.9) and the ladder _order_ stays right. What
breaks is the scale: the top player inflated to 2832 against a true skill of
2000, the bottom deflated to 144 against a true 1000.

**Why.** Not a random walk — it is far too consistent across seeds for that. It
is a selection effect. Pairing on a small _observed_ rating gap preferentially
picks pairs whose _true_ skill gap is larger, because a rating is a noisy
estimate of skill. The favourite therefore wins more often than the rating gap
predicted, gains points on average, and the spread inflates with every match.

**Evidence it is the pairing and not the update.** Same `settle` on both sides:
`rating.rs`'s test pairs at random and holds ~36 points of error over 40 000
matches; `sim.rs` pairs by rating and drifts as above. Two mitigations were
measured and neither fixes it — drawing the partner uniformly from the tolerance
band rather than taking the nearest changed nothing (2673 at 30k), and sending
2%/5%/15% of matches out wide as calibration got 30k error only to 277/232/164.

**The decision** was to move to Glicko-2, and it shipped on 2026-09-06 — see the
next section, which also records that the reason given for it here was the wrong
one.

Worth noting the drift is arguably _content_ for this sample rather than only a
defect: making a matchmaking property visible instead of asserted is what the
plan says the demo is for, and "your rating is only as good as the variety of
people you play" is a real thing to show. That does not settle which rating
system ships.

## Glicko-2 lands in bracket, and the lever is the step size (2026-09-06)

Record of the change that closed the section above. `apps/bracket/src/rating.rs`
is Glicko-2; the Elo K-factor schedule and `Rating::games` are gone.

**Where every constant came from.** All of them are Mark E. Glickman, _Example
of the Glicko-2 system_, Boston University, 22 March 2022
(`http://www.glicko.net/glicko/glicko2.pdf`), transcribed with the paper open
rather than recalled, except the last row.

| Constant                | Value                  | Paper                                      |
| ----------------------- | ---------------------- | ------------------------------------------ |
| `GLICKO2_SCALE`         | 173.7178               | steps 2 and 8                              |
| `Rating::START`         | 1500                   | step 1(a)                                  |
| `START_DEVIATION`       | 350                    | step 1(a)                                  |
| `TAU`                   | 0.5                    | step 1, recommended range 0.3–1.2          |
| `CONVERGENCE_TOLERANCE` | 0.000001               | step 5.1                                   |
| `START_VOLATILITY`      | 0.017320508 = 0.06/√12 | step 1(a)'s 0.06, rescaled — see below     |
| `PROVISIONAL_DEVIATION` | 110                    | **not the paper's**; a display convention  |
| `SKILL_SCALE`           | 400                    | not Glicko at all — the match stub's model |

**τ = 0.5 because the worked example is worked at it.** Any value in 0.3–1.2 is
allowed and the paper says to test for predictive accuracy; 0.5 is the middle of
that range _and_ the value its example prints numbers for, and those numbers are
the only thing in this codebase that can catch a transcription slip. Choosing
differently would have traded the one real falsifier for a tuning preference
nothing here can evaluate.

**One rating period per match**, which is the simplest mapping and the one a
live matchmaker wants — a result is rated when it is reported. The paper prefers
10–15 games per period, and that preference is not free; see the volatility
below. A player not in a match is not in a period either, so a deviation never
widens from sitting out: `Rating::idled` is the paper's rule for that case and
only `rate` with an empty period reaches it.

**Provisional is `deviation > 110`.** The paper names no threshold. 110 is built
on the one summary of RD it does give — 95% confident inside ±2 RD — so a
settled rating's interval reaches a fifth of the way across the 1000-point skill
range these populations span. It reads a rating and changes nothing about the
update.

**The measurement, 64 players, `mean_rating_error` and `rating_spread` against a
true skill range of 1000.** Elo is five seeds at HEAD `d6c4446`; both Glicko-2
rows are ten seeds.

| ticks   | Elo (K 40/20)          | Glicko-2, σ 0.06    | Glicko-2, σ 0.06/√12 |
| ------- | ---------------------- | ------------------- | -------------------- |
| 2 000   | error 54–58 / 978–1054 | 60–67 / 1400–1569   | 26–35 / 1127–1331    |
| 10 000  | 132–139 / 1750–1963    | 181–192 / 2046–2183 | 26–35 / 1150–1255    |
| 30 000  | 325–335 / 2683–2756    | 364–375 / 2786–2947 | 50–65 / 1263–1340    |
| 100 000 | not measured           | not measured        | 115–123 / 1466–1547  |

**The reason given for the move was wrong, and the middle column is the
evidence.** `g(RD)` was expected to be the correction: it attenuates the
expected score by how uncertain the _opponent's_ rating is. But it only bites
while a rating is uncertain, and a population that has played is not — the
deviation settles at 60.5 points at the paper's σ, where `g` is 0.982. Run that
way Glicko-2 drifts slightly _worse_ than the Elo it replaced, because at that
deviation the step it takes, 173.7·φ'², is 20.7 points against the
K-factor's 20.

What governs the drift is the size of that step, and Glicko-2's virtue is that
it derives it rather than being told it. The deviation settles where the
volatility's growth balances a period's worth of information, φ² ≈ σ·sqrt(v), so
the volatility is the knob — and it is the one constant step 1(a) explicitly
hands to the application ("this value depends on the particular application").
0.06 goes with the 10–15-game period the paper recommends; a period here is one
game, and the same per-period budget spread over one game instead of twelve is
0.06/√12 = 0.0173. That settles the deviation at 32.5 and is the third column.

**It is a six-fold reduction, not a cure.** The spread still climbs — 1127–1331
at 2 000 ticks to 1466–1547 at 100 000 — because the bias in each result is
still there and only its size changed. Two things would attack the bias itself
rather than its amplitude, and neither is scheduled: real 10–15-game rating
periods (which would also lower the equilibrium deviation to about the same 33
by a route the paper actually endorses, at the cost of a ladder that only moves
at period boundaries), and pairing that deliberately spends some matches wide,
which was measured on the Elo build and reported in the section above.

**Determinism.** `exp`, `ln`, `sqrt` and `powf` are the host's `libm`, so this
sample is reproducible from its seed on one host and not bit-identical across
hosts. That is not a regression — the Elo build ran `powf` for the same reason —
and nothing compares bracket's output across platforms: every assertion in the
crate is a bound, and the browser gate reads that `error:` is a number and that
matches are being played. bracket is not one of the crates named by
"Cross-target determinism: the CPU side takes `libm`".
