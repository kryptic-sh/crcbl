# Stage 5 — Physics

From-scratch physics engine (`crcbl-phys`), first-class engine pillar alongside
the server-authoritative core. Built for three headline capabilities: **galaxy-
scale worlds** (sector-tiled space, seamless streaming), **simulator-grade
dynamics** (real gravity, drag, terminal velocity), and **swept collision /
CCD** (fast movers never tunnel; hit registration composes with lag
compensation).

Physics is server simulation: `crcbl-phys` systems run in the server schedule,
own SoA arrays like every other system, and replicate results like any state.
The client never simulates (until the post-MVP prediction era, which reuses the
same deterministic core for rollback).

## Layered architecture

Each layer is shippable and useful alone; later layers never rewrite earlier
ones.

| Layer   | Contents                                                                                                                | MVP?                                                       |
| ------- | ----------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| **L0**  | Queries + kinematics: ray/segment/sweep/overlap, trigger volumes, character controller (capsule vs world, slopes/steps) | Yes                                                        |
| **L1**  | Forces + ballistics + orbits: gravity, drag, thrust, buoyancy; integrators; on-rails Kepler propagation                 | Yes                                                        |
| **CCD** | Swept collision for L0/L1 movers: TOI, motion-inflated broadphase                                                       | Yes                                                        |
| **L2**  | Contact solver: sequential impulses, warm starting, islands + sleeping                                                  | Stretch — may land parallel to stages 6–8; not an MVP gate |
| **L3**  | Constraints/joints (hinges, motors, vehicle-as-constraints)                                                             | Post-MVP                                                   |

Rationale: every MVP sample runs on L0+L1+CCD. **L2/L3 now have their own design
doc — see [36-contact-solver.md](36-contact-solver.md)** (substepped sequential
impulses, islands, sleeping, joints); they land wave 2, driven by ragdolls (35),
grenades, and dropped loot. L2 makes stacked/resting rigid bodies possible and
is the "generic 3D game" enabler — wanted soon, but nothing in the MVP ladder
blocks on it.

## Interleaved, demand-driven delivery

Physics is **not** built as one monolithic stage-5 block. The layers land as
vertical slices, each pulled in by the sample that needs it (sample rule 7 — all
collision/motion goes through `crcbl-phys`, no game-code collision math
anywhere):

| Slice                                                           | Demanded by | Layer  |
| --------------------------------------------------------------- | ----------- | ------ |
| Box/sphere colliders, swept-sphere TOI, contact normal response | breakout    | L0+CCD |
| Dynamic BVH churn, sphere overlap, segment CCD, thrust+damping  | asteroids   | L0+L1  |

**Slice 2 (P6) has landed**; `crates/crcbl-phys/src/lib.rs` is the inventory and
`git log` the history. Three things it did _not_ close are still owed, and
`docs/backlog.md` carries them:

- **Rotational dynamics.** `Transform` carries a `DQuat` and `ThrustForce` reads
  it, but `RigidBody` holds only mass, velocity and a force accumulator: no
  angular velocity, no torque, no inertia tensor anywhere in the crate. A body's
  orientation is set and never integrated, so the "full inertia tensor, torque,
  quaternion integration with renormalization" line below is unbuilt.
- A **teleport-aware re-insert** for screen wrap.
- A real `ShapeHit` from `PhysicsSystem::overlap_sphere`, which still returns a
  bare `Vec<Entity>`.

| Slice                                                        | Demanded by | Layer    |
| ------------------------------------------------------------ | ----------- | -------- |
| Batch overlap queries at 10k bodies, refit cost, sleeping    | horde       | L0       |
| Sector frames, gravity/drag/atmosphere, Kepler on-rails, SOI | orbit       | L1       |
| TOI vs moving targets, triggers, character controller        | towers      | L0+CCD   |
| Lag-compensated rewind queries                               | arena       | post-MVP |

**The orbit row has landed too**, and it is the largest of them: `Frames` with
`sphere_of_influence` crossings, `Atmosphere`/`AtmosphericDrag`,
`PointGravity`/`GravityForce`/`DragForce`, and `orbit::propagate` — a
universal-variable Kepler solve, so one path covers ellipse, parabola and
hyperbola. `apps/orbit` flies on it. **Bubbles and sleeping did not**: no type
in `crcbl-phys` names either, the broadphase is one BVH rather than one per
sector (`crcbl-phys`'s world has no sector partition at all), and what
`apps/orbit` calls "the bubble" is the sample's own on-rails/live switch rather
than an engine feature.

Stage 5 "exit" therefore overlaps stages 6–8 in wall-clock: the stage is done
when the full L0/L1/CCD surface exists and the orbit sample passes, not when a
calendar block ends. A physics feature no sample demands is a feature built too
early.

## Galaxy-scale space (with stage 1 foundations)

- **Sector-tiled positions** (lands in `crcbl-core` at stage 1, used
  everywhere): `WorldPos = { sector: I64Vec3, local: DVec3 }` — sparse 3D grid
  of sectors, f64 local offset, rebase on sector crossing (exact, cheap).
  Physics always computes in local/relative space; no absolute galactic float
  coordinates ever exist. The sector edge is `2^20 m` (~1048 km) — chosen as a
  real streaming/broadphase/interest cell (an FPS map fits in one, Earth spans
  ~12, a planet surface a few hundred), which forces the 64-bit index: a
  planet-scale cell only reaches galactic extent (`2^84 m` ≈ 2.04 billion ly per
  axis) with 64-bit sector coordinates.
- **Camera-relative rendering** (stage 3 note): instance transforms upload
  relative to camera sector+position each frame; GPU stays f32, no jitter.
- **Hierarchical reference frames**: bodies parent to dominant gravity source
  (galaxy → star → planet → moon → vehicle). Simulation runs in the local frame;
  the frame itself moves on-rails. Frame transitions (sphere-of- influence
  crossing) are explicit events.
- **On-rails vs live**: distant bodies = analytic Kepler orbits
  (`position = f(t)`, zero integration cost, stable forever). Live integration
  only inside **bubbles** around observers; server hosts multiple bubbles
  (multiplayer). Sleeping + on-rails is what makes galaxy scale cheap.
- **Streaming**: sector grid = streaming unit (async via `AssetSource`,
  load/unload by distance with hysteresis) = broadphase partition = interest-
  management key (stage 4 hook). One spatial structure, three consumers.

## Dynamics (L1)

- Fixed-timestep substeps under the server tick (60 Hz tick, 120–240 Hz substep;
  the stage 1 accumulator already provides this).
- **Symplectic (semi-implicit) Euler** default integrator; **RK4** or analytic
  Kepler for orbital propagation (plain Euler drifts orbits).
- Force pipeline in SI units, force providers are ECS systems appending into SoA
  force arrays: n-body gravity within a frame (dominant body + perturbations),
  atmospheric drag `F = ½ρv²·Cd·A` with exponential density-vs-altitude —
  terminal velocity **emerges**, not scripted; buoyancy; thrust; wind.
- Rotation: full inertia tensor, torque, quaternion integration with
  renormalization.
- **Determinism decision (made now)**: f64 math, same-binary/same-machine
  determinism, verified by per-tick state hash (stage 4 harness). Cross-platform
  lockstep (fixed-point core) is explicitly out — revisit only if a game demands
  it.

## Swept collision / CCD

- **Segment test** (projectiles/hitscan): prev→cur position segment vs
  broadphase. Day-one feature.
- **Swept shapes** (vehicles, fast bodies): sphere/capsule swept along the
  motion delta, analytic time-of-impact; advance to TOI, respond, spend
  remaining dt. Capsules cover ~90% of needs; convex-hull conservative
  advancement only if something forces it.
- **Broadphase**: dynamic AABB tree (BVH) per sector over motion-inflated
  ("fat", prev→cur) AABBs. Same tree serves L0 queries and editor picking (stage
  8 upgrades from plan-of-record AABB picking to phys raycasts).
- Speculative contacts evaluated as an alternative during L2 build; TOI
  substepping is the baseline.
- **Hit registration**: swept segment + lag-compensated rewind (arena sample) —
  server re-tests the shot in the world as the shooter saw it. Lookback in space
  composes with lookback in time.

## GPU split (round-trip principle applied)

- **Authoritative physics = CPU/server.** Gameplay reads results; GPU readback
  would poison the frame loop.
- **Visual-only physics = GPU compute** (debris, particles, eye candy):
  simulated in compute, rendered from device buffers, never read back,
  effectively unlimited count. The line: _does gameplay care? CPU. Eye candy?
  GPU._ Lands with/after stage 3 infrastructure, not gated on this stage.

## Debug tools (built with each layer)

- Debug draw: contacts, sweeps, fat AABBs, BVH bounds, islands, frame
  hierarchies, orbit paths.
- Per-tick physics state hash surfaced in the inspector; mismatch = loud.
- Time scrub: record N ticks of SoA state, scrub backward in the debug UI
  (replay is nearly free given determinism harness).
- Query visualizer: last N raycasts/sweeps drawn with hit points.

## Tasks

1. ~~`WorldPos`/sector types + rebase in `crcbl-core`~~ — shipped, with
   `WorldPos::relative_to` as the sanctioned conversion to render space. **The
   camera-relative upload was not executed in `crcbl-render`**: `relative_to`
   has no caller outside `crcbl-core`'s own tests, and
   `crates/crcbl-render/src/camera.rs` only documents that its `Vec3` is _meant_
   to be camera-relative. Nothing converts, so nothing has yet been far enough
   from the origin to jitter.
2. ~~`crcbl-phys` crate: SoA body storage, integrator, force-provider seam.~~
   Shipped: `RigidBody`, `SemiImplicitEuler`, `ForceProvider`.
3. ~~L0 queries: BVH, ray/segment/sweep/overlap, triggers, character
   controller.~~ Shipped, `CharacterController` included. **Static
   trimesh/heightfield is the one L0 shape still missing** — `crcbl-phys` has
   `Sphere`, `BoxCollider` and `Capsule` and nothing else.
4. ~~L1 forces: gravity/drag/thrust, atmosphere model, Kepler on-rails
   propagation, frame hierarchy + SOI transitions~~ — all shipped. **`buoyancy`
   was not**, and neither was the wind this document's dynamics section also
   names; they are the two force providers with no type.
5. ~~CCD: TOI sweeps, motion-inflated broadphase integration.~~ Shipped for
   sphere and capsule against sphere/box/capsule.
6. Bubbles + sleeping + sector streaming hooks.
7. Debug suite (draw, hash, scrub, query viz). **The hash is the only part
   built** — `PhysicsSystem::hash_state` feeds the tick hash;
   `PhysicsSystem::debug_draw` is an empty body with a "future" comment, and
   there is no scrub and no query visualiser anywhere.
8. (Stretch, non-gating) L2 sequential-impulse contact solver + islands.

## Exit criteria

- Orbit sample ([sample/06-orbit.md](sample/06-orbit.md)) passes: launch from
  planet surface, atmosphere exit (drag + terminal velocity measurably correct),
  stable orbit achieved with symplectic integrator (energy drift bounded over
  10k orbits on-rails handoff), deorbit + land, across at least one sector
  boundary with no visible seam. **`apps/orbit` is built and published, and it
  is not yet this criterion**: it carries `06-orbit.md`'s milestones 1 and 2 —
  ascent, orbit and timewarp — and its own header says the moon transfer is not
  flown and the bodies are drawn as a map rather than in 3D.
- Bullet-through-paper test: projectile at 10 km/s vs 1 cm wall, 100% hit rate
  via segment CCD; swept capsule vehicle at 500 m/s never tunnels terrain.
  Unbuilt: nothing in `crates/crcbl-phys/tests/` moves a body at anything like
  that speed (`dynamics.rs` is terminal velocity, thrust and damping;
  `broadphase_churn.rs` is tree churn), and the "vs terrain" half additionally
  needs the trimesh statics above.
- Character controller walks the towers map (slopes, steps, plot edges). towers
  does not exist. The controller does walk greybox courses instead —
  `apps/puppet`'s lane has steps and a pair of slopes deliberately straddling
  the walkable angle, and `apps/breach`'s firing line is a kerb the controller
  refuses — so slopes and steps are covered and editor-authored geometry is not.
- 1000-tick replay: state hash identical across 10 runs; scrub UI works. Half
  met — `crcbl sim` runs 1000 ticks and prints the hash, and `PhysicsSystem`
  contributes to it; there is no scrub UI.
- All physics visible in debug draw; no query without a visualizer. Unbuilt —
  `PhysicsSystem::debug_draw` is empty.

## Risks

- **Scope gravity (pun intended): L2/L3 pull.** The layer table is the contract
  — MVP gates on L0/L1/CCD only; solver work never blocks the sample ladder.
- **Sector/frame math edge cases** (rebase during sweep, SOI transition
  mid-substep). Mitigation: property tests with randomized boundary crossings;
  the determinism hash catches silent divergence.
- **f64 SIMD throughput** on wasm/older CPUs. Bubbles keep live-body counts
  bounded; profile before optimizing.

## Corrections (design review, 2026-07-27)

- **Static triangle-mesh / heightfield colliders were missing from the shape
  inventory** — yet the character controller walks editor-authored maps, orbit
  lands on terrain, 28 penetrates static meshes, and 24 voxelizes collider
  geometry. Added to L0 explicitly: **static trimesh with a BVH midphase** (plus
  segment/swept-capsule-vs-triangle in CCD), landing with the towers slice at
  the latest. Decision: **trimesh for statics**, convex decomposition only for
  dynamics that need it — 28's entry/exit penetration model depends on this
  choice, so it is made here rather than discovered.
- **Client-side query world (the client _does_ need collision)**: the 3P camera
  boom sweeps (30) and audio occlusion rays (13) are client-side and land well
  before prediction. The correct statement is "the client never **advances**
  simulation" — it hosts a **read-only query world** (statics from scene load +
  dynamic colliders reconstructed from snapshots, with interpolation-buffer
  staleness) for sweeps and rays. Owned as a P10 deliverable; previously nobody
  owned it.
- **Determinism scope, restated precisely**: f64 same-binary determinism was the
  original claim, but the module equivalence gate (16/P6A) and browser play
  require _cross-target_ agreement. Resolution: all determinism-bearing math
  routes through a **software transcendental implementation compiled into every
  target** (the `libm` crate is the standard answer) — never platform libm — and
  sim crates **ban FMA contraction** (no `mul_add`, no fast-math). Basic IEEE
  ops are already bit-exact everywhere. The scope is therefore "deterministic
  across targets _within the sim math kernel_", which is what the gates actually
  require.

## Correction (determinism math, 2026-08-09)

**This document and [13-audio.md](13-audio.md) prescribe different answers to
the same problem, and one of them has to win.** Both identify platform `libm` as
a cross-target determinism hazard; the correction above routes
determinism-bearing math through **the `libm` crate**, while topic 13's
correction requires **own polynomial approximations and LUTs** plus a CI deny on
std transcendentals inside `crcbl-audio`.

Neither is built. No workspace crate names `libm` — it reaches `Cargo.lock` only
through `naga` and `num-traits` and nothing in the sim path calls it — and there
is no deny anywhere. Recorded rather than silently resolved because the two are
not interchangeable: `libm` is a new dependency and therefore the user's call,
and hand-rolled approximations are a maintenance surface with their own
correctness burden. Both would need golden values from the specification, per
the verification rules in `12-testing.md`.

**The cost of leaving it open is now visible in the tree rather than
hypothetical.** `crates/crcbl-phys/src/character.rs` and
`crates/crcbl-phys/src/orbit.rs` each carry tolerances sized so a _different_
platform `libm` still passes — the orbit drift bound says so by name. Those
constants are the workaround this decision would remove; they are also what
would have to be re-derived once it is taken.

**Still unowned by any sample:** **static trimesh/heightfield colliders with a
BVH midphase**, per the 2026-07-27 correction. towers is the sample that demands
them. The **character controller** that stood beside them in this paragraph is
no longer outstanding — `CharacterController` ships, and `apps/puppet`,
`apps/breach` and `apps/shard` each drive it from a different camera rig.
