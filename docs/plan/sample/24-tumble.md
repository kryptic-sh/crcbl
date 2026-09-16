# Sample 24 — tumble (S4I, gates P8B)

Physics acceptance test and benchmark, and the demand driver for
[36-contact-solver.md](../36-contact-solver.md)'s rungs: a gallery of physics
scenes, natively and in a browser tab, each built to show one thing the engine
does — or, honestly, does not do yet — with the counters that measure it on the
screen.

**This is the sample that shows where the physics engine is weak, and by how
much.** The user's framing: building it "will show us where the gaps are in the
physics engine and its performance". So a scene whose behaviour the engine
cannot produce yet is not hidden or faked; it ships labelled as the gap it is,
and the rung that closes it turns the label off. The scene list is the solver's
acceptance suite, and the ball pit is its benchmark.

## Where the engine stands (surveyed 2026-09-15)

`crcbl-phys` detects collisions and responds to none. It has forces, queries,
sphere and Y-aligned-capsule sweeps against static shapes, a dynamic bounding
volume tree that survives spawn and despawn churn, and a character controller.
It has no rotation (`RigidBody` holds mass, velocity and a force accumulator),
no oriented boxes, no contact manifolds, no solver, no joints, no islands and no
sleeping; breakout's bounce is game code, and horde's ten thousand bodies are
kinematic sprites with no contacts. The published site runs physics on one
thread, because GitHub Pages sends no cross-origin isolation headers.
[36-contact-solver.md](../36-contact-solver.md)'s decisions of 2026-09-15 are
the plan that closes it.

## Proves

- **Each rung of the solver on the scene built for it** — the rung table in
  [36-contact-solver.md](../36-contact-solver.md) pairs every rung with a scene
  here, and the scene's counters are that rung's acceptance.
- **Performance at scale, measured**: the ball pit reports the most live bodies
  the engine holds inside a 60 Hz tick, natively and in the browser, with the
  broadphase, narrow-phase and solver cost of each tick on the page.
- **Determinism**: every scene is scripted and hashes the same on two runs, on
  every thread count, and — once the simulation's trigonometry is pinned —
  natively and in wasm.
- **Honest gaps**: until a rung lands, its scene says what the engine does
  instead ("balls pass through each other: no contact solver yet").

## Scope

The scenes, in the order the rungs make them real:

1. **The obstacle wall** (the user's first scene). A vertical board of static
   pegs, bars, wedges and bowls, and a spawner above it dropping balls, cubes or
   a mix. Exercises every shape pair, restitution and friction; its counters are
   contacts begun and ended, worst penetration and the bounce ratio.
2. **The ball pit** (the user's second scene). A pit with balls spawning in the
   centre without end; it overflows, balls roll off the edge and fall, and each
   despawns once it is a fixed distance from the scene origin — named as a
   constant and shown on the page. Its counters are live bodies, spawns and
   despawns per second, pairs, contacts, islands and sleeping bodies, every
   stage's time, the thread count and the most bodies held inside the tick.
3. **Zero-g tumbling.** A T-handle spinning about its intermediate axis, which
   flips — the Dzhanibekov effect is a test of the inertia tensor and gyroscopic
   torque — with angular momentum and energy drift on the page.
4. **Stacking.** A 20-box column, a base-20 pyramid and a domino run, with the
   top box's drift and the persisted contact-id ratio shown; flickering ids are
   the bug.
5. **Galton board.** Balls through a lattice of pegs into bins, with the bin
   histogram drawn against the binomial curve it should approach — a statistics
   check that fails visibly when restitution or friction is wrong.
6. **Settling.** Every earlier scene left alone reaches zero awake bodies, and a
   dropped ball wakes exactly the island it lands on.
7. **Bullets.** A cannon firing at thin plates and a brick wall, and a fast
   spinning plank, with a sensor behind the wall counting anything that
   tunnelled.
8. **Joints.** A gapped Newton's cradle with momentum in and out shown, a rope
   and chain bridge with crates rolling across, and capsule ragdolls down
   stairs.
9. **Pool and gale.** Crates and balls floating in a pool under gusts, with
   submerged fraction against Archimedes and drag from the wind field.
10. **Replay.** A pile's run recorded and replayed, with both hashes on the
    page.

- **Physics debug view**: contact points and normals, AABBs, islands coloured,
  sleeping bodies dimmed.
- **Server-authoritative**, as every game sample is: bodies are server state
  over the loopback, which also measures what replicating thousands of
  transforms costs.
- **Pages web demo** at `/demos/tumble/`, with the scene selector, the spawn
  rate, the body cap and the debug view as page controls, and the per-stage
  counters on the page.

## Non-goals (hard cap)

Soft bodies, cloth, fracture and fluids — a separate topic each. Vehicles.
Gameplay. Reduced-coordinate articulations, which
[36-contact-solver.md](../36-contact-solver.md) declines.

**Exempt from sample rule 11**, on lantern's ground. **Rule 9's "no game-code
collision math" is kept**: the first cut draws no bounce the engine does not
compute.

## Status: milestone 2 (Spin) built 2026-09-17

`apps/tumble` runs the zero-g T-handle and a box dropped flat, which falls
through the floor labelled "no contact solver yet". Milestone 1 and everything
after milestone 2 are not built.

## Milestones

1. **The honest first cut** — the user's choice of 2026-09-15: ship what the
   engine truthfully does, with the counters, before any solver exists. The ball
   pit as a spawn fountain with the despawn radius, balls falling under gravity
   and passing through each other, labelled "no contact solver yet", with spawn
   and despawn rates, live bodies and step time on the page; the bullet scene,
   which today's sphere sweep against a thin static plate can already answer;
   and a wind tunnel of spheres under a drag provider. The sample skeleton, the
   scene switch with the other scenes as labelled rooms, the web demo and the CI
   golden step.
2. **Spin** — rung 0: the zero-g T-handle.
3. **Pachinko** — rung 1: the obstacle wall with balls, and the ball pit with
   real contacts at a thousand balls.
4. **Tower** — rung 2: cubes on the wall, stacking, dominoes, the Galton board.
5. **Settle** — rung 3.
6. **Bullets** — rung 4, with the tunnelling sensor.
7. **Bridge** — rung 5: the cradle, the bridge, the ragdolls.
8. **Pit** — rung 6: the overflowing ball pit at full scale, the thread and SIMD
   hash checks, and the most-bodies figure natively and in the browser.
9. **Pool and gale** — rung 7.

## Exit criteria

- Every scene runs, and every rung's counters are on the panel, the headless
  summary and the page.
- Every scripted scene hashes the same on two runs and across thread counts;
  once the trigonometry is pinned, natively and in wasm.
- The base-20 pyramid stands for ten seconds with the top box's drift under the
  bound [36-contact-solver.md](../36-contact-solver.md) states, and the base-100
  pyramid is the benchmark figure.
- No tunnelling through the bullet scene's sensor below the stated speed limit.
- The ball pit's most-bodies figure recorded natively and in the browser.
- A golden per scene.
- Web demo deployed, running on the WebGPU backend.
