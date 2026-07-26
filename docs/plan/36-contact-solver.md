# Topic 36 — Contact Solver (Physics L2 + L3)

The rigid-body half of physics: contact generation, friction, restitution,
stacking, sleeping, and — with the same machinery — joints. Topic 5 layered this
as L2 (contacts) and L3 (constraints) and left it as a paragraph because nothing
in the MVP demanded it. Ragdolls (35), grenades, dropped loot, and vehicles do,
so it gets a real design here.

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

## Delivery (wave 2 — before ragdolls, which consume it)

1. Mass properties, body kinds, manifold generation, contact IDs.
2. Substepped sequential-impulse solver with soft constraints + warm starting;
   friction/restitution from materials.
3. Islands + deterministic ordering + `par_for` + sleeping.
4. `KineticContact` emission from solver impulses (completes 28's contact half).
5. L3 joints (fixed/hinge/cone/6-DOF) + limits/motors/breakable.
6. Debug draw, profiler rows, stability suites.

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
