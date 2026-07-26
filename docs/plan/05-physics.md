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

Rationale: every planned sample runs on L0+L1+CCD. L2 makes stacked/resting
rigid bodies possible and is the "generic 3D game" enabler — wanted soon, but
nothing in the MVP ladder blocks on it.

## Interleaved, demand-driven delivery

Physics is **not** built as one monolithic stage-5 block. The layers land as
vertical slices, each pulled in by the sample that needs it (sample rule 7 — all
collision/motion goes through `crcbl-phys`, no game-code collision math
anywhere):

| Slice                                                           | Demanded by | Layer    |
| --------------------------------------------------------------- | ----------- | -------- |
| Box/sphere colliders, swept-sphere TOI, contact normal response | breakout    | L0+CCD   |
| Dynamic BVH churn, sphere overlap, segment CCD, thrust+damping  | asteroids   | L0+L1    |
| Batch overlap queries at 10k bodies, refit cost, sleeping       | horde       | L0       |
| Sector frames, gravity/drag/atmosphere, Kepler on-rails, SOI    | orbit       | L1       |
| TOI vs moving targets, triggers, character controller           | towers      | L0+CCD   |
| Lag-compensated rewind queries                                  | arena       | post-MVP |

Stage 5 "exit" therefore overlaps stages 6–8 in wall-clock: the stage is done
when the full L0/L1/CCD surface exists and the orbit sample passes, not when a
calendar block ends. A physics feature no sample demands is a feature built too
early.

## Galaxy-scale space (with stage 1 foundations)

- **Sector-tiled positions** (lands in `crcbl-core` at stage 1, used
  everywhere): `WorldPos = { sector: IVec3, local: DVec3 }` — sparse 3D grid of
  sectors, f64 local offset, rebase on sector crossing (exact, cheap). Physics
  always computes in local/relative space; no absolute galactic float
  coordinates ever exist.
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

1. `WorldPos`/sector types + rebase in `crcbl-core` (stage 1 backfill if not
   already landed), camera-relative upload note executed in `crcbl-render`.
2. `crcbl-phys` crate: SoA body storage, integrator, force-provider seam.
3. L0 queries: BVH, ray/segment/sweep/overlap, triggers, character controller.
4. L1 forces: gravity/drag/thrust/buoyancy, atmosphere model, Kepler on-rails
   propagation, frame hierarchy + SOI transitions.
5. CCD: TOI sweeps, motion-inflated broadphase integration.
6. Bubbles + sleeping + sector streaming hooks.
7. Debug suite (draw, hash, scrub, query viz).
8. (Stretch, non-gating) L2 sequential-impulse contact solver + islands.

## Exit criteria

- Orbit sample ([sample/06-orbit.md](sample/06-orbit.md)) passes: launch from
  planet surface, atmosphere exit (drag + terminal velocity measurably correct),
  stable orbit achieved with symplectic integrator (energy drift bounded over
  10k orbits on-rails handoff), deorbit + land, across at least one sector
  boundary with no visible seam.
- Bullet-through-paper test: projectile at 10 km/s vs 1 cm wall, 100% hit rate
  via segment CCD; swept capsule vehicle at 500 m/s never tunnels terrain.
- Character controller walks the towers map (slopes, steps, plot edges).
- 1000-tick replay: state hash identical across 10 runs; scrub UI works.
- All physics visible in debug draw; no query without a visualizer.

## Risks

- **Scope gravity (pun intended): L2/L3 pull.** The layer table is the contract
  — MVP gates on L0/L1/CCD only; solver work never blocks the sample ladder.
- **Sector/frame math edge cases** (rebase during sweep, SOI transition
  mid-substep). Mitigation: property tests with randomized boundary crossings;
  the determinism hash catches silent divergence.
- **f64 SIMD throughput** on wasm/older CPUs. Bubbles keep live-body counts
  bounded; profile before optimizing.
