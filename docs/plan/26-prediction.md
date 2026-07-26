# Topic 26 — Client Prediction + Lag Compensation

The netcode endgame: client-side prediction with rollback/reconciliation, and
server-side lag-compensated hit registration. Everything before this was staged
for it deliberately — tick sync (21), input tick rings (21), snapshot rings
(23), determinism hashing (12), module equivalence (16). This topic spends what
those saved. Wave 2, arena = driver; a competitive-shooter project pulls it to
front-of-line.

## Client prediction

### What predicts

- **Opt-in per system** via a `predicted` flag in the component schema —
  typically the player's own movement (char controller), weapon state, and
  locally-owned projectiles' first ticks. Everything else stays interpolated
  (the MVP path, unchanged).
- **Same sim code runs client-side**: predicted systems execute the identical
  module/system code (static or wasm — the P6A equivalence gate is why this is
  trustworthy) on the client at tick `N = server_tick + lead` (topic 21).
  Prediction is not a parallel implementation — it's the real simulation, scoped
  to a predicted subset. No game-code changes beyond the schema flag.
- **Split timeline, standard model**: own entity rendered at predicted _now_;
  remote entities rendered at interpolated _past_ (~100 ms buffer). The engine
  owns the bookkeeping; games see one world.

### Rollback + reconciliation

1. Client stores per-tick: predicted state of the predicted subset (ring, last
   ~1 s) + `InputTickState`s (the topic 21 input ring — built for exactly this).
2. Authoritative snapshot for tick M arrives → compare against stored prediction
   at M (per-component epsilon from the schema; exact for integers/flags).
3. Match → discard history ≤ M, done (the overwhelmingly common case).
4. Mismatch → **rollback**: restore authoritative M, re-simulate M+1…N replaying
   stored inputs — bounded work (predicted subset only, ≤ lead+jitter ticks,
   typically 2–6 re-sims of a few systems).
5. **Error smoothing**: the _visual_ transform blends the correction away over
   ~100–200 ms (decaying offset between sim state and rendered state) — sim
   snaps exactly, the camera never teleports. Visual-error state is client-side
   presentation, same category as interpolation.

- Misprediction sources are accepted, not fought: another player shoved you, a
  server event intervened — rollback handles all of them uniformly because it
  replays _authoritative truth + your inputs_, not guesses.

### Cosmetic prediction (feel without lies)

Muzzle flash, fire sound, first tracer frame play **immediately** on the
predicted fire input — cosmetic-only, no state. Server confirmation drives
consequences (hit markers, damage numbers, kill feed: confirm-only by default —
per-game policy knob, because "predicted hit markers that lie" is a
competitive-integrity choice games must own).

## Lag compensation (server-side rewind, query-only)

The shooter saw the world ~(interpolation buffer + ½RTT) in the past; the server
validates shots **in that past**:

- **Hitbox history ring**: per tick, the server stores combat-relevant hitbox
  transforms (compact: entity id + posed hitbox set), last ~250–500 ms. This is
  the third use of the per-tick ring machinery (snapshots 23, replay 22, now
  hitboxes) — same pattern, tiny payload.
- **Animation-posed hitboxes without server skinning**: clips cook **per-clip
  hitbox transform tracks** (topic 17 bake grows this) — the server samples
  hitbox poses from its anim _state_ (which it already ticks) with zero pose
  math. Tarkov-grade limb accuracy at server cost of a table lookup.
- Shot commands carry the client's view time (interpolated tick T + fraction,
  which the client knows exactly). Server: clamp T to the max rewind window →
  interpolate stored hitboxes to T → run the full ballistic query (CCD segment +
  penetration — the FPS phys extension) in the rewound hitbox world + _present_
  static world → apply results at present.
- **Query-only rewind (LOCKED)**: authoritative state never rewinds; only the
  hit test looks backward. No state rollback server-side, no rewind of physics,
  no re-simulation — lag comp is a read.
- **Fairness knobs are config, stated openly**: `max_rewind_ms` (high-ping
  advantage cap), whether movers-behind-cover honor the shooter's or the
  victim's timeline (the peeker's-advantage tradeoff) — engine provides the
  mechanism + the measurement harness; each game owns the policy.
- Slow projectiles need none of this: they're simulated entities (spawn may
  optionally rewind-offset the origin one window; policy knob).

## Debug + measurement (the feature is unfinishable without these)

- **Netcode HUD** (extends the topic 23 netgraph): corrections/sec, rollback
  depth histogram, prediction error magnitude (pre-smoothing), smoothing
  residual, rewind window usage per shot.
- **Rewind visualizer**: server debug-draws the rewound hitboxes it tested
  against (and records them into the replay — disputed kills are _reviewable_ in
  the topic 22 scrub debugger with both timelines shown).
- Condition-simulator presets (topic 23) drive A/B: prediction on/off at 100 ms
  RTT is arena's recorded before/after.
- **Fairness harness**: scripted bot duels at asymmetric RTTs (arena's bots +
  navigation) → hit% vs ping curves; flat-within-tolerance is the acceptance
  metric, not vibes.

## Testing (topic 12)

- Rollback idempotence property: rollback + replay with identical inputs ≡ never
  having rolled back (state hash).
- Zero-divergence property: with no packet loss and no external influence,
  corrections/sec = 0 over a full input script (catches false mispredictions =
  epsilon bugs or nondeterminism leaks — and `crcbl replay verify` distinguishes
  which).
- Loss/jitter soak: condition-simulator sweeps in CI; bounded rollback depth
  asserted.
- Lag-comp golden cases: scripted shooter/target/RTT scenarios with known
  expected hits (including edge: target just-behind-cover at each policy
  setting).
- Cross-binding: predicted systems pass the wasm/static equivalence gate (16) —
  prediction works identically for module games.

## Delivery (wave 2 — arena; front-of-line for a competitive-FPS project)

1. Predicted-subset rings + rollback re-sim (own movement predicts; arena
   feels-right milestone at 100 ms).
2. Error smoothing + netcode HUD.
3. Hitbox history ring + cooked hitbox tracks + query-only rewind;
   CCD/penetration query integration.
4. Fairness harness + policy knobs + replay-integrated rewind visualizer.
5. Cosmetic-prediction policy surface (confirm-gated consequences).

## Risks

- **Nondeterminism leaks into prediction** → constant micro-corrections: the
  zero-divergence CI property catches it structurally; `replay verify` locates
  it (that tool pays for itself here).
- **Re-sim cost spikes** (deep rollback under bad jitter): bounded window +
  predicted-subset-only keeps worst case small; HUD histogram makes regressions
  visible.
- **Policy wars (peeker's advantage etc.)**: engine ships mechanism +
  measurement, never a hidden opinion — knobs are documented config with harness
  numbers attached.
- **Scope creep toward full-world prediction**: predicted subset is
  schema-declared and reviewed; "predict everything" is a rejected architecture
  (that's lockstep's territory, a different engine).

## Corrections (design review, 2026-07-27)

- **Compare in encoded space, not sim space** (the false-misprediction
  generator): the server simulates f64 but the wire carries quantized values, so
  comparing a f64 prediction against a decoded authoritative value produces
  constant "mismatches". The client **quantizes its predicted state through the
  same wire codec** before storing and comparing — both sides live in encoded
  space (the Overwatch/Rocket League practice). With 23's identity codec at P2
  this costs nothing now and prevents a P13 rewrite.
- **Lag-comp rewind must not trust client-reported view time** — clamping only
  to `max_rewind_ms` _is_ the CS:GO-style backtrack cheat (always claim the
  oldest legal T). The server **independently estimates** each client's view
  time from its known interpolation-buffer depth + measured RTT and clamps the
  reported value to that estimate ± a small tolerance; `max_rewind_ms` is the
  outer bound, not the validator. This is a security requirement, not a fairness
  knob.
- **Rollback collision context (was undecided)**: re-simulated ticks collide
  against the **latest authoritative snapshot held fixed** for the whole re-sim
  (the standard choice), not per-tick historical snapshots and not statics-only.
  Stated so it isn't discovered mid-implementation.
- **The predicted-state capsule includes physics-internal state**: character
  controller ground/contact caches, solver warm-start impulses for predicted
  bodies, and **the per-tick RNG stream position**. Without these, "restore
  authoritative M" is not a restore and rollback drifts silently.
