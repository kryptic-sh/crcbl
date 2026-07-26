# Sample 07 — orbit

KSP-lite slice: launch a rocket from a planet surface, punch through the
atmosphere, reach stable orbit, deorbit, land. One planet, one moon, one rocket.
The physics stage's acceptance test as a playable toy.

## Proves

- **Sector-tiled galaxy coordinates**: planet→orbit→moon transfer crosses sector
  boundaries and reference-frame transitions (surface frame → planet frame →
  moon SOI) with no visible seam or jitter — camera-relative rendering +
  `WorldPos` rebase demonstrated end-to-end.
- **Simulator-grade dynamics**: thrust vs gravity vs drag; terminal velocity
  observable on descent; atmosphere density model felt in gameplay (drag
  heating/velocity readouts, not effects); orbital mechanics real (symplectic
  integration live, Kepler on-rails when coasting far from the bubble).
- **On-rails ↔ live handoff**: timewarp while coasting (on-rails), drop to live
  integration on burn — the KSP-model bubble architecture, playable.
- **L0 in anger**: landing legs = swept capsule contacts with terrain;
  character-free sample keeps the controller out of scope here (towers covers
  it).
- **Debug tools as instruments**: orbit path debug draw, apoapsis/periapsis
  readouts from the propagator, physics scrub after a crashed landing.

## Scope

- One star, one planet (atmosphere + terrain patch at landing sites), one moon
  (vacuum). Rocket = single stage, pitch/yaw/throttle, fuel budget.
- Flight UI: altitude, velocity (surface/orbital), apo/peri, fuel, simple
  navball-lite (prograde/retrograde markers).
- Timewarp ×1–×1000 (on-rails only; auto-drop on burn/atmosphere).
- Win condition: land on the moon and return. Crash = restart.
- Solo only; server-authoritative rule still applies (sim on server, in-memory
  transport — timewarp is a server command).

## Non-goals (hard cap)

Staging, construction/VAB, multiple bodies beyond planet+moon, n-body beyond
dominant+perturbation, re-entry heating damage, life support, career anything.
This is a physics acceptance test wearing a rocket costume.

## Milestones

1. Suborbital hop: launch, drag, terminal-velocity descent, land (L1 + CCD vs
   terrain).
2. Stable orbit + timewarp (integrator quality + on-rails handoff).
3. Moon transfer: SOI transition + sector crossing (frame hierarchy).
4. Land + return; polish readouts.

## Exit criteria

- Full mission (surface → orbit → moon landing → return) completable by a
  patient human with the flight UI only.
- Orbit stable over 10k timewarped periods (energy drift bound recorded in doc).
- Physics numbers sanity-checked against real-world formulas in tests (terminal
  velocity for given Cd/A/ρ within 1%, orbital period for given altitude within
  0.1%).
- Sector boundary crossing invisible at max warp and live rates alike.
