# Sample 06 — orbit

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
- **`.crpix` art for the flight UI's chrome and the map view** (sample rule 11)
  — the navball-lite, the prograde/retrograde markers, the apo/peri glyphs. The
  bodies and the rocket are 3D and stay 3D; the 2D layer over them is sprites,
  not hand-placed quads. **Debug panel on** (rule 4): timewarp ×1000 is a frame
  budget question before it is a physics question.

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

## Where this stands

**Milestones 1 and 2 are built**, and none of the physics is this sample's own.
`apps/orbit` flies the ascent, reaches a stable orbit and timewarps along it
with the auto-drop, over an `InMemoryTransport` with the flight as a
`GameModule` the authoritative server owns — rule 2 has no exemption for a
physics demo and this sample takes none, so timewarp is a command the client
sends rather than a rendering trick. The pieces underneath are all
`crcbl-phys`'s: `PointGravity`, `Atmosphere` and `AtmosphericDrag` under
`SemiImplicitEuler` for live integration, `Frames` and `sphere_of_influence` for
the reference-frame hierarchy, and `propagate` for the on-rails arc. What is
this sample's is the vehicle, the controls and the flight plan.

**The planet is sized for a game rather than for Earth**, on Kerbal Space
Program's own reasoning — orbital velocity at Earth is 7.8 km/s and a real
ascent is eight minutes of burn, which is a fixture nobody would fly twice.
`apps/orbit/src/game.rs` argues it where the constants are.

**And it flies itself until the player takes the controls.** A page that has
just loaded takes no input, and a rocket standing on a pad is indistinguishable
from a stopped loop, so a script flies the gravity turn and the circularisation
burn and the first thing the player asks for ends it for good — the same
arrangement `apps/viewer` and `apps/puppet` use.

Not built: **milestones 3 and 4.** The moon's frame exists and a ship that
reached it would be handed over, but nothing flies there yet. The bodies are
drawn as a **map view** over the flight instruments rather than in 3D, so the
Scope's "no visible seam or jitter" claim has not been looked at in a 3D frame.
And the `.crpix` art rule 11 asks for is not there — `apps/orbit` has no
`build.rs` and no `assets/`, and the navball-lite, the prograde/retrograde
markers and the apo/peri glyphs are drawn as rectangles, polylines and text.
That is rule 11 owed rather than exempted; this doc claims no exemption and
should not be read as taking one.

## Exit criteria

- Full mission (surface → orbit → moon landing → return) completable by a
  patient human with the flight UI only.
- Orbit stable over 10k timewarped periods (energy drift bound recorded in doc).
- Physics numbers sanity-checked against real-world formulas in tests (terminal
  velocity for given Cd/A/ρ within 1%, orbital period for given altitude within
  0.1%).
- Sector boundary crossing invisible at max warp and live rates alike.
