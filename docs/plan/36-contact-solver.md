# Topic 36 — Contact Solver (Physics L2 + L3)

The rigid-body half of physics: contact generation, friction, restitution,
stacking, sleeping, and — with the same machinery — joints. Topic 5 layered this
as L2 (contacts) and L3 (constraints) and left it as a paragraph because nothing
in the MVP demanded it. Ragdolls (35), grenades, dropped loot, and vehicles do,
so it gets a real design here.

**Status: rungs 0 and 1 built (2026-09-17); rung 2 built for boxes, not for
general hulls (2026-09-23); rung 3 built (2026-09-23).** Rung 0: rotation
(inertia tensor, torque, the implicit-midpoint gyroscopic step), dense
generational body sets, `SurfaceMaterial` per body and `crcbl_core::trig`. Rung
1, opted into with `PhysicsSystem::with_contacts`: split fattened broadphase
trees with a move buffer and pair set; analytic sphere and capsule manifolds
against spheres, capsules, oriented boxes and planes, plus box against plane,
with feature ids; and a substepped soft solver with warm starting, speculative
contacts and a restitution pass, raising `KineticContact`. Two measured
departures from the sections below: **the speculative distance grows with the
pair's closing speed**, because decision 5's fixed four slops let a 30 m/s ball
through a 2 cm plate, and **separation within a tick is tracked to first order**
rather than through Box2D's turned anchors, which made a rolling ball slip.
Sphere against sphere is in rung 1 because the ball pit needs it.

Rung 2, for boxes: box against box, static or dynamic, through Ericson's
fifteen-axis separating axis test with the pair's last axis cached (`SatCache`),
Gregorius's face-over-edge tolerances, Sutherland–Hodgman clipping, reduction to
four points and flip-invariant feature ids naming a feature of each box; and
friction at each manifold's centroid with a twist term. Measured: a base-20
pyramid of one-metre cubes holds for ten seconds at the defaults, its top box
sunk 2.76 cm and 0.05 mm off sideways, 4 points a manifold, never under 99.7% of
ids persisted; the Tower room's base-20 pyramid takes 766 µs a tick in the
solver and 186 µs in the narrow phase, in a release build on a Ryzen 9 9950X3D,
one thread, scalar `f64`. Four departures. **Sphere and capsule against a box
stay analytic** rather than GJK with a SAT fallback: against a box the closest
point is a clamp, exact and cheaper, and GJK arrives with the hull it is for —
though rung 2 fixed two rung 1 cases where a capsule along a box edge, or across
a face and past its end, rested on one wandering point, and named the box's
feature in those pairs' ids. **The 20-box column does not stand at decision 1's
30 Hz**: a soft contact's stiffness is `m ω²` whatever it carries, so a column
buckles under its own weight past Greenhill's height, `(1.96 ω² w / g)^⅓` cubes
of half-extent `w` — fifteen one-metre cubes at 30 Hz, and measured, fourteen
stood and seventeen fell. The column runs `ContactSettings::TALL_STACK`, eight
substeps at 90 Hz, for its whole system until substeps are per group. **Fast
spinners still sink** — a 5 cm cube at 80 rad/s turned a corner 6.9 cm into a
peg on the wall — because rotation outruns a once-a-tick manifold, which is rung
4's. **Not built: general convex hulls** (there is no hull collider) and GJK for
spheres and capsules against them.

Rung 3, in `crates/crcbl-phys/src/contact/island.rs`: persistent islands of
dynamic bodies, merged when a contact between two of them begins touching and
split lazily — one a tick, the island whose sleepiest body has been still
longest, and only once some of it could sleep; an island sleeps when every body
has stayed under 0.05 m/s and 0.1 rad/s for 0.5 s, its bodies moving out of the
awake set into the island, with velocities zeroed, as Box3D's per-island sets
do. Every wake rule of decision 4 that has something to wake is built: a contact
beginning touching with an awake body, an applied force or torque, a velocity or
body written, a teleport, a new collider or material, and a touching neighbour's
removal; a query wakes nothing. Measured, in a release build on the same Ryzen 9
9950X3D: a base-20 pyramid of half-metre cubes asleep at tick 59, its solver
time a mean 850 µs a tick over its first thirty ticks and 1.6 µs over thirty
asleep; in the Tower room the pyramid asleep at tick 58 and the column and
dominoes at 276; the thousand-ball pit asleep at tick 1000, its solver a mean 22
µs a tick over the next ten seconds, the first tick of them still awake; the
wall 326 ticks after its spawner stops. Five departures. **Turning is judged by
angular speed**, where Box2D judges it by the speed of the body's farthest
point: the two agree for half-metre props, and angular speed keeps a spinning
body with no collider awake. **A kinematic body wakes what it touches only while
it moves**, and a body it touches while moving cannot sleep; kinematic and
static bodies join no island. **A static body placed or created on a sleeper
wakes it**, a rule decision 4 does not list, since the narrow phase never
collides two bodies neither of which moves. **Contacts stay in one pool**: a
sleeping island's contacts are skipped on a look at their records rather than
moved out, which costs the pit at rest a mean 96 µs a tick in the narrow phase,
measured the same way. **A stack sleeps before it is still**: under 5 cm/s is
not stopped, so the Tower room's pyramid sleeps with its top cube 2.4 mm aside,
where ten seconds awake would have crept it back to 0.39 mm. Decision 4's "a new
joint" waits for joints. Nothing from rung 4 on: sweeps or joints.

Compound bodies, built 2026-09-23 for EW's dropped items, and not a rung:
`ColliderComponent::Compound` carries a `CompoundShape` of up to
`CompoundShape::MAX_PARTS` boxes fixed in the body's frame, each optionally
turned, and "Bodies"'s "compound bodies supported" is met this way. **Each part
is a broadphase proxy of its own**, as each shape of a Box2D v3 body is: two
parts of one body never pair, and every contact is a part pair's, so the
separating axis cache, feature ids and warm starting apply per part pair
unchanged, and islands and sleep see only bodies. The declined alternative, one
proxy per body and a narrow phase walking part pairs with ids widened by part,
would repeat the broadphase's cull every tick and give a contact several
normals. A body pair's points are bounded by four per touching part pair, and
the part cap bounds the pairs; there is no reduction across a body pair's
contacts. Mass and inertia sum the parts by the parallel-axis theorem at one
density, counting an overlap once per part, as Box2D, Rapier and Jolt do. The
query world holds one box around a compound's parts. Measured, EW's TOZ-34 boxed
into six parts falls 30 cm tumbling onto a static slab, lands on its side and
sleeps at tick 47; see `crates/crcbl-phys/tests/compounds.rs`.

## Decisions from the engine research (2026-09-15)

The user delegated the solver family to research — "do some deep research on the
best physics engines and make the decisions based on the research" — after
deciding that physics stays built from scratch and that a physics showcase
([sample/24-tumble.md](sample/24-tumble.md)) drives it. The brief read Box2D
v3's and Box3D's source and posts (Erin Catto), Jolt Physics's source and its
Horizon Forbidden West talk, Rapier 0.35, PhysX 5.6, Avian, Bepu v2, Bullet,
MuJoCo, Macklin et al.'s _Small Steps_ and Gregorius's contact-creation talks.
**This section supersedes the sections below where they disagree**, and says
which.

**The finding that matters most**: Box2D v3, Box3D (June 2026), Rapier 0.35
(August 2026) and Avian have converged on one design — _islands exist for
sleeping only, and the awake set is solved as one pool split into persistent
constraint colours_ whose constraints never share a body. Box2D's own source
says it in a sentence: "Solver using graph coloring. Islands are only used for
sleep". Jolt instead rebuilds islands each step, sorts each for determinism and
splits the large ones; it ships at AAA scale, and it is the runner-up.

1. **Solver: Soft Step, for contacts and joints** — which the section below
   already names in spirit, now with its parameters written down. Collision runs
   once per tick; the solver runs **4 substeps**, each with **1 biased iteration
   and 1 relax iteration**, warm-started from the previous tick, with no
   convergence loop. Contacts are soft at 30 Hz with damping ratio 10 (static
   contacts at twice that), push-out capped at 3 m/s; joints at 60 Hz with
   damping 2; speed capped at 400 m/s and rotation at π/4 per substep.
   **Restitution is its own pass after the substeps**, above a 1 m/s threshold —
   Rapier moved it there because speculative contacts damped bounces. A long
   chain or bridge gets more substeps for its group, not more iterations.
   _Evidence_: Catto's Solver2D comparison, where Soft Step needs 4 constraint
   passes for the quality plain PGS reaches in 8; Macklin 2019's substeps over
   iterations; adoption by Box2D, Box3D, Rapier, Avian and PhysX's TGS.
   _Declined_: XPBD — Avian left it because deep overlap was explosive, it never
   truly settled, friction was weaker and collision ran every substep, and was
   4–6× faster after switching; Catto reports friction and far-from-origin
   precision failures. Jolt-style PGS with a position solve works but needs 10 +
   2 iterations.
2. **Narrow phase: analytic pairs, and SAT for boxes and hulls, with no EPA.**
   Sphere, capsule and their pairs are analytic. Boxes are hulls: a separating
   axis test cached per pair, face clipping, reduction to at most four points
   (deepest, then maximum area, with hysteresis), and **flip-invariant feature
   IDs** for warm starting. Sphere or capsule against a hull uses GJK on the
   core shape with a SAT fallback when deep. Friction acts at the manifold's
   centroid with a twist term, as Box3D and Rapier do. _Declined_: GJK/EPA with
   convex margins (Jolt, PhysX's PCM) — visible gaps, and warm-start points
   matched by distance rather than by feature. **Replaces "SAT/GJK-EPA" below.**
3. **Broadphase: keep the SAH/AVL tree, split and fattened.** A static tree and
   a dynamic tree, the static one rebuilt after load; fat margins of min(5 cm, ⅛
   of the extent); a move buffer so only enlarged proxies query; a pair set;
   persistent contacts created before they touch. The ball pit's churn is an
   O(log n) insert and remove per ball, which Box3D's rain benchmark exercises
   the same way.
4. **Islands, sleeping and parallelism: islands for sleep, colours for
   solving.** Islands are persistent — merged on a contact beginning, split
   lazily, the sleepiest one per tick — and sleep as a whole after 0.5 s below
   0.05 m/s. Awake bodies live in dense arrays and sleeping ones move out. Awake
   constraints are coloured greedily with per-colour body bitsets, persistently,
   with an overflow colour. **Order comes from the persistent arrays, never a
   per-step sort**, and parallel narrow-phase results merge through per-worker
   bitsets, so `crcbl-jobs` schedules work without deciding its order: the
   single-threaded Pages build runs the same stages in the same colour order and
   hashes the same. A body wakes on a contact beginning with an awake body, an
   applied impulse, a new joint, or **a touching neighbour's removal** (Jolt
   does not wake on removal, which is a trap to test); **a query does not wake
   anything**. **Replaces "Islands solve independently → par_for … ordered by
   lowest entity id … contacts sorted per island" and "a query touching it"
   below.**
5. **Continuous collision: speculative contacts for everything, then sweeps for
   fast bodies.** Contacts are created within four times the linear slop; after
   the solve, a body that moved more than half its smallest extent sweeps
   against statics (a bullet flag adds dynamic and kinematic bodies), and lost
   time is dropped rather than re-solved — single-pass, as Box2D, Jolt and PhysX
   practice. The existing sphere and capsule sweeps are reused; hulls use GJK
   conservative advancement. The 10 km/s projectile stays
   [28-ballistics.md](28-ballistics.md)'s segment test. **Inverts "CCD stays
   L0/L1's job: fast movers sweep to their TOI, then the solver resolves" below,
   and [05-physics.md](05-physics.md)'s "TOI baseline, speculative
   alternative".**
6. **Joints: impulse joints in the same solver** — revolute, spherical with cone
   and twist limits, distance and rope, weld, prismatic; limits, motors,
   breakable by impulse; capsule ragdolls, which is
   [35-ragdolls.md](35-ragdolls.md)'s simplified server ragdoll. _Declined_:
   reduced-coordinate multibodies — tree-only (a bridge needs loop-closing
   constraints anyway), slow to add and remove, no joint forces for breaking,
   and a second solver.
7. **Precision: f64 positions and an f32 solver interior — decided by the user,
   2026-09-17.** Body positions stay f64 in sector-local space: f32 resolves
   only 0.125 m at a 2²⁰ m sector's edge. The solver already works on deltas and
   anchors relative to each body, so its interior (velocities, deltas, impulses,
   effective masses) can be f32. The argument for switching at rung 6 is width:
   WebAssembly's SIMD offers two f64 lanes against four f32 lanes, Jolt measured
   a naive all-double build at over 2× slower against 5–10% for its boundary
   design, and Box2D reports "a few percent". f32 is exactly as deterministic as
   f64; only the hash differs, so every target uses one precision. **This amends
   [05-physics.md](05-physics.md)'s locked "f64" line.** The simulation's `sin`
   and `cos` are constructed in-engine in f64 on `crcbl_shaders::trig`'s
   pattern, also decided 2026-09-17, which is what rung 0's "pinned
   trigonometry" means.
8. **Data layout: dense and generational, not hash maps.** `PhysicsSystem` keeps
   bodies and transforms in hash maps keyed by entity and sorts the keys each
   step (`crates/crcbl-phys/src/system.rs`), which cannot carry a contact graph
   or a wide solve. The target is Box3D's: generational body ids mapped to (set,
   index), the entity map only at the ECS boundary; a cold body record and a hot
   body state; contacts in a persistent pooled array with per-body edge lists
   and a colour index; constraints prepared each tick into per-colour
   struct-of-arrays blocks whose scalar path does the same per-lane arithmetic,
   so scalar and SIMD builds hash the same, as Box2D's CI proves. Hashing
   canonicalises −0.0 and NaN; no `mul_add`, no relaxed SIMD.
9. **Tests grow to the benchmarks' size.** The regression pyramid's base is 20
   in CI and 100 (5050 boxes) as the benchmark, matching Box2D's and Box3D's
   large pyramid; energy never rises; penetration stays under the slop; the hash
   is equal across thread counts, SIMD and scalar, native and wasm.

**Measured elsewhere, for pricing** (Box3D's benchmarks, a Ryzen 7950X, 60 Hz, 4
substeps): the 5050-box pyramid at about 10.4 ms per step on one SSE2 thread, 25
ms scalar, and 1.7 ms on eight threads.

## Algorithm: substepped sequential impulses

The Catto/Box2D lineage, in its modern (soft-constraint, substepped) form — the
best-documented from-scratch territory in physics, and the one whose failure
modes are known in advance:

- **Substepping** (the biggest single quality win): run N small solver substeps
  per tick rather than one big step with many iterations. Stiff stacks and
  joints converge dramatically better for the same budget, and it composes with
  our fixed-timestep tick (4/21) — substep count is a knob, not a mystery.
- **Velocity-level sequential impulses** per substep: iterate contacts, apply
  corrective impulses along normal and friction directions until velocities
  satisfy the constraints.
- **Soft constraints** (spring-damper formulation with tunable
  stiffness/damping) instead of raw Baumgarte bias — removes the classic
  "penetration correction adds energy" artifact.
- **Warm starting**: contact impulses are cached across ticks keyed by
  **persistent contact IDs** (feature pairs from the manifold), and applied as
  the first guess next tick. This is what makes a stack of crates stand still
  instead of shivering — non-negotiable, in from the first slice.
- **Speculative contacts** (contacts created slightly before touching, using the
  CCD sweep distance from L0) prevent tunneling inside the solver without a
  separate pass.

## Contacts

- **Manifold generation** per broadphase pair (the BVH from L0): SAT/GJK-EPA for
  convex pairs, sphere/capsule/box fast paths, with reduced contact sets (≤4
  points for a face-face manifold) and stable point IDs across frames.
- **Materials from the collider property block** — the same block that carries
  acoustic (13), nav (24) and ballistic (28) properties gains friction and
  restitution. One material asset per surface, four consumers, no parallel
  tables.
- Combination rules for pairs (multiply/average/max, per-property, data- driven)
  — the standard escape hatch for "ice on rubber".
- Contact impulses above a threshold emit `KineticContact` (28) — the damage
  path is a byproduct of solving, not a second collision system.

## Islands, sleeping, and parallelism

- **Islands** = connected components of touching/jointed bodies. Each island
  solves independently → the jobs pool (21) runs islands in `par_for`
  **deterministic mode**.
- **Determinism under parallelism (LOCKED)**: islands are built and ordered by a
  stable key (lowest entity id), contacts within an island sorted by (body pair,
  feature id), iteration counts fixed. Same result at `--threads 1` and
  `--threads N` — the topic 21 killer test covers the solver from its first
  commit, not later.
- **Sleeping**: an island whose bodies stay below linear/angular thresholds for
  N ticks sleeps and costs nothing; woken by a new contact, an applied impulse,
  a query touching it, or a neighbour waking. This is what makes 40 corpses and
  a floor of dropped loot free (35, 34) — sleeping is a first-slice feature, not
  an optimization added later.

## Bodies

- **Mass properties** computed from collider shapes (density → mass, inertia
  tensor), compound bodies supported, explicit overrides allowed; center of mass
  separate from origin.
- Body kinds: **dynamic** (solved), **kinematic** (moved by game/animation,
  infinite mass to the solver — the character controller and spline-followers
  already work this way), **static** (world geometry).
- Damping, gravity scale, max velocity clamps (the anti-explosion seatbelt), and
  per-body sleep thresholds are data.

## L3 joints — the same solver

A joint is a constraint with a different Jacobian; nothing new is needed
structurally:

- **Types**: fixed, hinge (1 DOF + limits + optional motor), swing-twist cone
  (ragdoll shoulders/hips), prismatic/slider, distance/spring, 6-DOF generic
  with per-axis lock/limit/motor.
- Limits and motors are constraints in the same iteration loop; breakable joints
  (force threshold → detach event) come free and are useful for destructible
  props later.
- **Consumers**: ragdolls (35), doors and hatches, vehicle suspension
  (post-MVP), swinging props, weapon slings.

## Interaction with the rest of physics

- **CCD stays L0/L1's job**: fast movers sweep to their TOI, then the solver
  resolves contacts at that position — no fighting between systems.
- **Character controller stays kinematic** (5): it queries and sweeps, it is not
  solver-driven; dynamic bodies react to it via one-way pushes with a
  configurable force budget (the standard "player can shove crates but a crate
  can't launch the player" rule).
- **Vehicles** are joints + wheels-as-raycasts, post-MVP, listed so the solver's
  requirements are known in advance.

## Debug + tooling

- Debug draw: contact points/normals/impulse magnitudes, manifold IDs
  (warm-start continuity is _visible_ — flickering IDs are the bug), islands
  colored, sleep states, joint frames with limit cones.
- Profiler rows: broadphase, manifold gen, solver (per substep), islands
  count/size histogram, sleeping ratio.
- `crcbl phys stack --check` — scripted stability scenarios headless.

## Testing (topic 12)

- **Stacking stability**: a 10-box pyramid stands N seconds with drift below a
  bound (the canonical solver regression test).
- **Energy property**: total energy never increases without applied impulses
  (catches bias/restitution bugs — the same invariant ragdolls rely on).
- **Penetration bound**: steady-state overlap stays under the slop threshold; no
  tunneling for bodies up to the CCD velocity limit.
- Analytic cases: restitution (drop height → bounce height within tolerance),
  friction (incline angle vs slide/stick per material pair).
- **Determinism**: 1000-tick stack + ragdoll scenes hash-identical across thread
  counts and runs.
- **Sleeping**: an idle pile consumes ~zero solver time and wakes correctly on
  contact, impulse, and query.

## Delivery (revised 2026-09-15: rungs, each with the scene that proves it)

Each rung lands with a scene in [sample/24-tumble.md](sample/24-tumble.md) and
the counters that scene displays. Sleep follows contacts and boxes rather than
arriving with parallel islands; SIMD and parallelism come last.

| Rung                                | Scope                                                                                                                                                                                                                                              | Proving scene                                                                             | Counters                                                                                                                                        |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| 0 Spin (built)                      | Inertia tensors, quaternion integration, gyroscopic torque; dense solver sets replacing the hash maps; friction and restitution from materials ([37-materials.md](37-materials.md)); pinned trigonometry for the simulation                        | a zero-g tumbling T-handle; a box dropped flat                                            | angular momentum and energy drift, step time, hash                                                                                              |
| 1 Pachinko (built)                  | Analytic sphere and capsule manifolds against static boxes and planes; split trees, move buffer, pair set, persistent contacts; scalar Soft Step with warm starting, speculative contacts and the restitution pass; `KineticContact` from impulses | the obstacle wall with falling balls; a thousand-ball pit                                 | bodies, pairs, contacts begun and ended; broadphase, narrow-phase and solver time; worst penetration; bounce ratio                              |
| 2 Tower (built for boxes; no hulls) | Boxes and hulls: cached SAT, clipping, four-point reduction, feature ids; GJK with SAT fallback for spheres and capsules against hulls; centroid and twist friction                                                                                | a 20-box column, a base-20 pyramid, dominoes, cubes on the wall                           | points per manifold, persisted-id ratio, top-box drift                                                                                          |
| 3 Settle (built)                    | Persistent islands, lazy splitting, island sleep, the wake rules                                                                                                                                                                                   | every earlier scene settles to zero awake bodies                                          | islands, awake and sleeping bodies, solver time at rest                                                                                         |
| 4 Bullets                           | Fast-body sweeps against statics, the bullet flag, dropped time                                                                                                                                                                                    | a cannon at thin plates and a brick wall; a fast spinning plank                           | sweep candidates, hits, tunnels through a sensor behind the wall                                                                                |
| 5 Bridge                            | The joint framework and types, limits, motors, breaking, extra substeps per group; a static triangle mesh with active-edge handling before the stairs                                                                                              | a gapped Newton's cradle, a rope and chain bridge with crates, capsule ragdolls on stairs | joint error, bridge sag, cradle momentum in and out, broken joints                                                                              |
| 6 Pit                               | Persistent colouring with overflow, the wide solver kernel with its scalar twin, staged `crcbl-jobs` execution, contact recycling                                                                                                                  | the overflowing ball pit with its despawn radius, and cube rain                           | spawns and despawns per second, colours, overflow, stage times, threads, the hash across threads and targets, most bodies inside a 16.7 ms tick |
| 7 Pool and gale                     | Buoyancy ([55-water.md](55-water.md)) and wind ([56-wind.md](56-wind.md)) force providers                                                                                                                                                          | crates and balls in a pool under gusts                                                    | submerged fraction, depth against Archimedes, drag                                                                                              |

## Risks

- **Tuning rabbit hole**: solver quality is a tuning surface with no natural
  end. The stability suites define "good enough" numerically, and substepping
  buys quality that iteration-count fiddling cannot.
- **Determinism vs parallel islands**: solved by construction (stable ordering)
  and enforced by the threads-1-vs-N hash test from the first commit —
  retrofitting determinism into a solver is far harder than building it in.
- **Scope creep**: soft bodies, cloth, fracture, fluids are _not_ here. L2/L3 is
  rigid bodies and joints; anything deformable is a separate topic with a
  separate justification.
- **Character-vs-dynamics expectations**: the kinematic controller rule ("player
  pushes crates, crates don't launch player") is documented so it reads as a
  decision, not a bug.
