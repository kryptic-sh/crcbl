# Topic 28 — Ballistics + Kinetic Impact

The kinetic-harm layer of physics (topic 5): penetrating projectile sweeps **and
generalized impact transfer** — one model where **anything with mass and
velocity can hurt anything it hits**. A bullet, a hammer dropped five stories, a
vehicle, your own body meeting the ground: all of them resolve through the same
physics (kinetic energy in, energy deposited on contact) and report through one
event shape. The engine owns the **kinetic transport** (paths, penetration,
deposited energy/impulse); the game owns the **damage model** (health, armor
semantics, energy→damage curves) on top. FPS-project era for the ballistic half;
contact impacts ride L2; designed now so the collider property block, round
model, and event schema grow the right shape.

## The kinetic model: mass × velocity, everywhere

Projectiles are not abstract "damage rays" — a round is a physical object:

```ron
Round(
  mass: 0.008,        // kg — 8 g rifle round
  muzzle_velocity: 900.0, // m/s → E = ½mv² ≈ 3.2 kJ, momentum p = mv
  pen_coeff: 1.8,     // penetrator hardness/shape factor (AP > FMJ > soft)
  area: 0.6,          // cm² presented cross-section (sectional density input)
)
```

- **Energy** `E = ½mv²` drives harm potential; **penetration capability** =
  `E × pen_coeff / area` (sectional density: heavy-narrow-hard penetrates,
  light-wide-soft transfers) vs material `resistance × thickness`.
- The same numbers describe a hammer (2 kg, terminal velocity from a 5-story
  fall via L1 gravity/drag ≈ 17 m/s → ~290 J, huge area, pen ≈ 0) or a car (1500
  kg at 8 m/s → 48 kJ, pen 0). Nothing special-cases them — low `pen_coeff/area`
  just means the penetration branch never wins and everything becomes
  deposition.

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

## Energy deposition: penetration and blunt are one accounting

Every surface interaction **deposits** energy into what was hit — the chain
reports it per hit, and deposition is the universal damage input:

- **Penetrated**: deposited = `resistance × thickness` (what the material
  absorbed slowing the round); the rest continues.
- **Stopped/embedded**: deposited = _all_ remaining energy — which is exactly
  **blunt transfer**. A plate that stops a round eats 3 kJ; the game's damage
  model maps deposited-behind-armor energy (minus the armor's attenuation
  factor, game data) to trauma. Behind-armor blunt damage isn't a feature bolted
  on — it's the energy conservation the model already does.
- **Ricochet**: deposited = the energy lost in the bounce.

## Generalized kinetic contacts: the `KineticContact` event

One event schema unifies every way mass-in-motion meets a body:

```ron
KineticContact(
  source: Ballistic | Contact,   // sweep chain hit vs solver contact
  bodies: (impactor, struck),    // entity ids (impactor may be None: world)
  point, normal,
  relative_velocity, impactor_mass,
  energy_deposited, impulse,     // the two damage-relevant magnitudes
)
```

- **Ballistic sweeps** emit one per chain hit (as above).
- **Contact impulses** (L2 solver, and CCD/controller contacts before L2): any
  collision whose impulse exceeds a per-body threshold emits one —
  resting/rolling contacts filtered by relative-velocity floor, so no event spam
  from a crate sitting on the floor.
- This makes the requested cases fall out with **zero special systems**:
  - _hammer from the 5th floor_: L1 gravity+drag gives impact velocity → CCD
    contact on the head hitbox → `KineticContact { ~290 J }` → game's
    energy→damage curve says ouch;
  - _vehicle hits player_: solver contact impulse from a 1500 kg body → same
    event, very large numbers;
  - _fall damage_: your own body contacting ground **is** a `KineticContact`
    (struck = you, impactor = world) — fall damage is this system consumed by
    the game, not an engine special case;
  - _thrown crate, debris, melee swing_: mass × velocity, same event.
- Deterministic like all sim: solver impulses and sweep chains are tick-state →
  events replicate, replay, and hash like everything else.

## Hitboxes + armor: the engine/game split

- **Every chain hit and `KineticContact` carries
  `(entity, collider_id, collider_tag)`** — guaranteed, so game damage models
  always know _what part_ was struck (hitbox→body-part mapping is a game table
  keyed on the tag). No damage system ever guesses from geometry.
- **Nested hitbox colliders are supported**: hitbox sets may contain child
  colliders (organ boxes) attached to the same skeleton transforms (the topic 26
  cooked hitbox tracks pose them together, lag-comp rewinds them together). The
  penetrating sweep traverses them in order like any other surfaces — flesh →
  organ → flesh, each with its own deposition. **A Tarkov-grade organ-damage
  model is therefore pure game data**: organ colliders + per-organ components +
  an energy→trauma table; the engine contribution is that a liver shot is
  _geometry_, not a dice table.
- Character hitboxes are colliders with the `flesh` material — limbs cost
  energy, through-and-through exits happen naturally, multi-limb paths report
  each hit with its entry energy **and deposition**.
- **Armor is game logic**: the query returns the ordered hit chain with
  energy/deposition at each surface; the game's damage module resolves armor
  (class/durability/blunt-through — Tarkov semantics live in game data), and
  **truncates the chain** at a stopping hit (ignore hits after k) — the stopping
  hit's deposition is the blunt-trauma input. Flesh resistance is low, so
  pass-through energy error from truncation is negligible — this keeps the query
  pure and batchable, no game callbacks inside the physics loop. The engine
  ships the transport and the joules; games ship the hurt.

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
- **Kinetic-contact goldens**: hammer 5-story drop deposits `≈ mgh − drag`
  within tolerance; vehicle-vs-hitbox impulse matches momentum math;
  behind-armor deposition = stopped energy exactly (conservation asserted: Σ
  deposited + retained + ricochet losses = initial E, per chain).
- Threshold behavior: resting/rolling contacts emit nothing; the event floor is
  deterministic (no jitter-dependent damage ticks).
- Fuzz: degenerate geometry (coplanar exits, zero-thickness, grazing hits) never
  NaN/hang — chain terminates cleanly.

## Delivery (FPS-project era; API shape reserved in topic 5 now)

1. Multi-hit ordered traversal + material resistance + embed + per-hit
   deposition (the core, energy-conserving from day one).
2. `KineticContact` event + CCD/controller contact emission (falls, drops,
   thrown objects — pre-L2).
3. Ricochet + exit deflection (deterministic rolls).
4. Drag media volumes.
5. Armor interface (chain truncation contract) + lag-comp composite query.
6. L2 solver impulse emission (vehicles, dynamic-body impacts) — lands with L2
   contacts (wave 2 dependency).
7. Shot-trace viz + `penmatrix` + golden-slab + kinetic-golden suites.

## Risks

- **Exit-geometry edge cases** (open meshes, nested colliders, coplanar faces):
  content rule + `assumed_thickness` fallback + fuzz suite; the visualizer makes
  bad geometry obvious in seconds.
- **Balance thrash masquerading as engine bugs**: penmatrix in CI turns material
  edits into reviewable diffs — tuning is data with a paper trail.
- **Realism creep** (temperature, bullet deformation, spall simulation): the
  energy/resistance model is the contract; higher fidelity is game-side
  interpretation of the same chain, not more physics.

## Corrections (design review, 2026-07-27)

- **RNG consumption order is ABI-stable.** The client predicts a shot's exact
  spread _and_ its ricochet/deflection rolls from the same seed (38); if the
  predicted and authoritative queries draw from the stream in a different order
  or count, tracers diverge permanently. The per-shot draw sequence is therefore
  part of the ballistics contract, versioned with the ABI, and covered by a
  property test comparing predicted vs authoritative chains.
- **Multi-tick projectile lag compensation is decided**: a round in flight
  rewinds **per segment with a sliding window** — segment _k_ tests against
  hitboxes at `shot_view_time + elapsed_flight_time`, i.e. the world as it was
  when the round reached that point (the Battlefield-style approach), clamped by
  the same `max_rewind_ms`. Hitscan-like rounds collapse to the single-segment
  case; slow projectiles converge on present-time. Added to the fairness harness
  scenarios.
