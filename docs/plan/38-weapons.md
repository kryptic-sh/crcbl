# Topic 38 — Weapon Kit

Weapons touch more engine systems than anything else — ballistics (28),
animation (17), audio (13), VFX (20), viewmodel/optics (29), inventory and
attachments (34), prediction (26), input (19). Every shooter rebuilds the same
fire/reload/recoil state machine and gets the same three things wrong
(client-authoritative fire rate, ammo duplication, attachment stat composition).
So it ships as a **kit**: `crcbl-weapons`, optional and privilege-free like the
player kit (30) and inventory kit (34) — a TD never registers it; breach starts
from it.

**None of it is built.** There is no `crcbl-weapons` crate, no weapon asset
schema and no resolved stat block; ballistics (28) has no material model to
compose against and the inventory kit (34) has no items to make a magazine out
of. `apps/breach` ships today at its own milestone 0 and does not anticipate
this kit: its pistol is one `crcbl_phys::PhysicsWorld::cast_ray` per shot with
no round, no penetration, no spread pattern and no state machine. Its rate limit
is the trigger arriving as an input **edge** — one press, one shot — with no
server-owned next-allowed-shot tick and no RPM at all. So the "three classic
exploits, closed" section describes work not started rather than a property
breach holds.

## A weapon is data over systems that already exist

A weapon definition is a RON asset composing references, not a new subsystem:

```ron
Weapon(
  item: (footprint: (5,2), tags: ["primary"]),        // 34: it's an item
  rounds: "762x39_fmj",                                // 28: mass/velocity/pen/area
  handling: (
    modes: [Semi, Auto], rpm: 600,
    spread: Curve(...), bloom: Curve(...),             // per-shot growth + decay
    recoil: "ak_pattern",                              // deterministic, learnable
    ads_time_ms: 260, reload_ms: 2400, tactical_reload_ms: 1900,
  ),
  attachments: [                                       // 34: 1x1 filtered grids
    (id: "optic",     filter: "optic"),
    (id: "muzzle",    filter: "muzzle"),
    (id: "magazine",  filter: "mag_762"),
  ],
  presentation: (
    world_mesh: "...", viewmodel_mesh: "...",          // 29: two models
    sockets: (muzzle: "s_muzzle", sight: "s_sight", eject: "s_eject", mag: "s_mag"),
    anim_set: "rifle_std",                             // 17: state machine + events
    audio: "ak_events", vfx: "rifle_muzzle",           // 13 / 20
  ),
)
```

Ammunition and magazines are **items** (34) — a mag is a container with a
capacity, loaded rounds are stacked items in it, so "check your mag" is an
inventory query and ammo counting is the inventory's anti-dupe discipline, not a
special integer.

## Attachment stat composition (the bit everyone gets wrong)

- Each attachment declares an ordered **modifier list**: `ads_time ×0.90`,
  `recoil ×0.85`, `muzzle_velocity +5%`, `audio_profile = "suppressed"`,
  `socket_override(sight = "s_optic_eye")` (29's ADS alignment realigns by
  socket math automatically).
- **One composition rule, stated once**: multiplicative modifiers multiply,
  additive add, replacements take the last writer in a declared attachment order
  — deterministic and order-stable, computed **server-side** into a resolved
  stat block that is cached and replicated. Games tune values, not the algebra.
- Modifiers reach into other systems by data, not code: ballistics params (28),
  audio event set (13), viewmodel sockets (29), even weight (34 → encumbrance in
  30).

## Server-authoritative firing (the three classic exploits, closed)

- **Fire rate is server-enforced**: the server owns the next-allowed-shot tick;
  a client firing faster is ignored, not trusted. Rate-of-hire hacks become
  no-ops rather than bans.
- **Ammo is inventory state**: rounds leave the magazine as an atomic inventory
  transaction (34) — no client-reported counters, no dupes.
- **Spread/recoil are server-computed** from the deterministic pattern + seeded
  RNG `(tick, shooter, shot_index)` — the same seeding as ballistics (28), so
  the client can _predict_ the exact same spread it will be judged by. No "my
  crosshair said otherwise" divergence.
- Client prediction is **cosmetic-first** (26): muzzle flash, sound, first
  tracer frame, viewmodel kick fire instantly on input; hits, damage numbers and
  ammo decrement confirm from the server.

## Recoil as a learnable pattern

Same philosophy as the audio cue grammar (13): recoil is **deterministic and
learnable**, not random spray. A pattern is authored data (a curve of per-shot
kick offsets + a recovery curve); mastering it is a skill, and the pattern
visualizer (below) is a first-class tool rather than something players
reverse-engineer from YouTube. Randomness, where a game wants it, is a declared
jitter band on top — visible in the same visualizer.

## States (the machine nobody should rewrite)

Idle → fire (per mode) → cycle → reload (tactical vs empty) → inspect →
malfunction/jam → unjam, plus deploy/holster and ADS as a parallel layer. The
kit provides the machine; games supply timings, anim sets, and whether
malfunctions exist at all (Tarkov yes, CS-like no). Timings are driven by
**animation events** (17), so audio, VFX, and state changes land on the same
frame across 1P/3P/spectator (29's full-sync guarantees).

## Tooling

- **Weapon panel** (editor): stat sheet with attachments simulated live (equip
  an optic, watch the resolved block change), socket preview, anim-event
  timeline.
- **Recoil pattern visualizer**: plots the pattern, overlays actual shot traces
  from a test burst — authoring and verification in one view.
- **Balance artifacts as CI outputs** (the penmatrix pattern from 28):
  `crcbl weapon stats` emits a table of weapons × resolved stats, and
  `crcbl weapon ttk` a time-to-kill matrix against armor classes — so a balance
  change is a reviewable diff, not a vibe.

## Testing (12)

- **Fire-rate property**: a client spamming fire commands never exceeds RPM in
  server state (fuzzed timing, incl. clock-skew attempts).
- **Ammo conservation**: rounds fired + rounds remaining + rounds in world =
  rounds loaded, across reloads and mag swaps (rides 34's no-dupe fuzz).
- **Composition determinism**: attachment permutations resolve identically
  regardless of equip order (the property that makes stat bugs impossible to
  reproduce otherwise).
- Spread/recoil determinism: same seed → same pattern, client prediction matches
  server judgement exactly.
- Anim-event timing: audio/VFX/state changes fire on the intended tick at any
  framerate (17's event test, applied to weapons).

## Delivery (FPS-era; breach is the driver and the fixture)

1. Weapon asset schema + item/attachment wiring (34) + resolved stat block.
2. State machine + anim-event-driven timings + fire modes.
3. Server-authoritative fire gate, ammo transactions, seeded spread/recoil.
4. Cosmetic prediction wiring (26) + viewmodel/socket integration (29).
5. Attachment modifiers incl. audio profile + ballistic params + optics.
6. Weapon panel, recoil visualizer, `stats`/`ttk` CI artifacts.

## Risks

- **Kit vs game boundary**: the kit stops at the machine, the stat algebra, and
  the authority rules. Weapon _balance_, unlock/economy, and skins are game data
  — the same line the player kit holds.
- **Stat-composition sprawl**: modifiers are a declared vocabulary; a game
  wanting a modifier the kit lacks adds it to its own layer rather than bending
  the algebra.
- **Feel is subjective**: recoil/spread tuning is endless. The visualizer + TTK
  matrix make it measurable, and breach playtests decide.
