# Topic 28 — Ballistic Penetration

The penetrating-projectile extension to physics CCD (topic 5): multi-surface
segment queries with material-based energy loss, ricochet, deflection, and media
drag — the engine half of Tarkov-grade gunplay. The engine owns the **ballistic
transport** (where the projectile goes, what it passes through, how much energy
survives); the game owns the **damage model** (health, armor semantics) on top
of the query results. FPS-project era; designed now so the collider property
block and query API grow the right shape.

## Material model (the collider property block grows again)

Colliders already carry surface properties (acoustic — 13; nav flags — 24).
Ballistic fields join them, preset-based like acoustic materials:

```ron
BallisticMaterial(
  resistance: 45.0,        // energy cost per cm of path through the material
  ricochet: Curve(...),    // ricochet probability vs incidence angle × velocity
  deflection: 0.8,         // exit deviation scale (cone half-angle factor)
  assumed_thickness: 2.0,  // cm — fallback when geometry can't provide an exit
  shatter: false,          // glass-style: first hit destroys occluder (event)
)
```

Presets: drywall, wood, sheet-metal, brick, concrete, steel, dirt, glass, flesh;
**water/volumes are drag media**, not surfaces (below). Same learnability
contract as the audio grammar: material → consistent, versioned behavior —
players learn what walls do.

## The query: penetrating sweep

Extends the CCD segment/sweep (topic 5) from first-hit to **ordered entry/exit
traversal**:

1. Collect hits along the segment in `t` order. **Convex colliders give entry
   _and_ exit from one intersection test** (slab thickness ≈ free); penetrable
   static meshes must be closed or convex-decomposed — a stated content rule;
   violations fall back to `assumed_thickness` (loud in the debug viz, never
   wrong-crash).
2. At each surface, deterministic resolution in order:
   - **Ricochet roll** from the material curve (incidence angle × impact
     velocity). Ricochet → reflect with energy loss + deterministic scatter,
     continue as a new segment (bounce count capped).
   - **Penetrate**: `E_out = E_in − resistance × thickness_along_path` (path
     length through the material, so oblique hits cost more — angled walls stop
     more, for free). `E_out ≤ E_min` → projectile embeds, path ends.
   - **Exit deflection**: small deterministic cone deviation scaled by
     `deflection × thickness` — long paths through dense material wander.
3. Continue from the exit point with reduced velocity; repeat. Bounded:
   `max_surfaces` (default ~4) + max total path length.

- **All randomness** (ricochet, scatter, deflection) draws from the
  deterministic RNG (topic 16 host import) seeded by
  `(tick, shooter, shot_index)` — replay/verify-safe, lockstep with the hash
  pillar, and identical under lag-comp re-query.
- **Drag media**: water/foliage/gel volumes aren't surfaces — path segments
  _inside_ them decay velocity continuously (per-medium drag coefficient, the
  topic 5 L1 drag model applied along the ray). High-drag media (water) stop
  rifle rounds in ~a meter, correctly, from the same physics.
- **Batch API** like every phys query: N shots in, N hit-chains out;
  `par_for`-friendly (MG fire, shotgun pellets = one batch).

## One query, both projectile kinds

- **Fast rounds** (rifle-class): per-tick ballistic step — the projectile is
  simulated (L1 gravity + drag between ticks, real flight time at long range)
  and each tick's path segment runs the penetrating sweep. Within typical combat
  ranges this collapses to a single-tick "hitscan-feel" segment; at long range,
  drop and lead emerge from the same code.
- **Slow projectiles** (grenades, rockets): same entities, same per-tick step;
  penetration usually irrelevant (they bounce/detonate — material `resistance`
  just stops them).
- **Lag-comp integration** (topic 26): the hit-validation query runs the
  identical penetrating sweep against rewound hitboxes + present statics.
  Wallbangs are lag-compensated by construction.

## Hitboxes + armor: the engine/game split

- Character hitboxes are colliders with the `flesh` material — limbs cost
  energy, through-and-through exits happen naturally, multi-limb paths report
  each hit with its entry energy.
- **Armor is game logic**: the query returns the ordered hit chain with energy
  at each surface; the game's damage module resolves armor
  (class/durability/blunt-through — Tarkov semantics live in game data), and
  **truncates the chain** at a stopping hit (ignore hits after k). Flesh
  resistance is low, so pass-through energy error from truncation is negligible
  — this keeps the query pure and batchable, no game callbacks inside the
  physics loop. The engine ships the transport; games ship the hurt.

## Debug + tooling

- **Shot-trace visualizer** (debug draw + recorded into replays, pairing with
  the topic 26 rewind viz): full path with entry/exit markers, energy graph
  along the path, ricochet branches, media segments — a disputed wallbang is
  fully reviewable in the scrub debugger.
- `crcbl phys shoot --from --dir --round <preset>` CLI: fire test shots
  headless, dump the hit chain as RON; `crcbl phys penmatrix` — table of round
  presets × material presets → exit energies (the balance-tuning artifact,
  regenerated in CI so material edits show their blast radius).
- Round presets (velocity, energy, E_min) are game data (RON), engine consumes.

## Testing (topic 12)

- Golden slabs: material × thickness × angle grid → exit energy/deviation within
  tolerance bands (the penmatrix, asserted).
- Properties: energy strictly non-increasing along any chain; path bounded;
  oblique ≥ perpendicular cost; same seed → identical chain (determinism).
- Media: submerged-segment decay matches closed-form drag integral.
- Lag-comp composite: golden wallbang scenarios through rewound hitboxes.
- Fuzz: degenerate geometry (coplanar exits, zero-thickness, grazing hits) never
  NaN/hang — chain terminates cleanly.

## Delivery (FPS-project era; API shape reserved in topic 5 now)

1. Multi-hit ordered traversal + material resistance + embed (the core).
2. Ricochet + exit deflection (deterministic rolls).
3. Drag media volumes.
4. Armor interface (chain truncation contract) + lag-comp composite query.
5. Shot-trace viz + `penmatrix` + golden-slab suite.

## Risks

- **Exit-geometry edge cases** (open meshes, nested colliders, coplanar faces):
  content rule + `assumed_thickness` fallback + fuzz suite; the visualizer makes
  bad geometry obvious in seconds.
- **Balance thrash masquerading as engine bugs**: penmatrix in CI turns material
  edits into reviewable diffs — tuning is data with a paper trail.
- **Realism creep** (temperature, bullet deformation, spall simulation): the
  energy/resistance model is the contract; higher fidelity is game-side
  interpretation of the same chain, not more physics.
