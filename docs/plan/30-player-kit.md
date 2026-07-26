# Topic 30 — Player Kit: Controller + Camera Suite

A first-party, batteries-included player controller with 1P and 3P camera rigs
(GTA-template third person) — the Source-2-style "every game gets a working
character for free" layer. Nearly every FPS/3PS needs exactly this system; a
tower defense or RTS needs none of it — so it ships **in the engine as a
first-class kit, but strictly optional**: built entirely on public seams and
module-available APIs, adopt it wholesale, replace pieces, or ignore it
completely (towers/hud never register it; puppet and any future shooter start
from it). It holds no engine privileges — the dogfood rule (same as the editor)
is what guarantees the roll-your-own path stays viable.

## Structural position

`crcbl-player` = a shipped **module/kit**, not engine core: movement runs as an
ordinary server system (predicted-flagged, 26) over the phys character
controller (5 L0); cameras are client presentation components; input is a
standard action set (19); views ride topic 29's machinery. `crcbl new` templates
register it by default; deleting one line unregisters it. If the kit needs a
private engine hook to work, that's an engine API gap to fix — never a special
door.

## Movement controller (server system, prediction-ready)

- Capsule character controller consumer (5): walk/run/sprint/crouch(/prone as
  data), jump with coyote-time + buffered-jump knobs, slope limits, step offset,
  acceleration/friction curves — **all tuning is a RON preset** (ground accel,
  air control, sprint multipliers…). Default preset = responsive third-person
  action tuning; presets are swappable per game (a "milsim" preset ships too —
  heavier, inertia-weighted, Tarkov-feel).
- Server-authoritative + `predicted` (26) out of the box — the kit is the
  reference consumer of the prediction pipeline.
- Emits the standard locomotion params the anim state machine (17) and footstep
  events consume — pairing with puppet's blend trees by default.

## Camera rigs

### First person

Thin binding over topic 29: eye-socket camera, ADS hooks, bob/sway procedural
layers — the kit wires it, 29 defines it.

### Third person (GTA template)

The classic follow-cam, implemented properly once:

- **Spring-arm boom**: desired position = pivot (spine/head socket) + orbit
  offset (yaw/pitch from look input) + shoulder offset; a **phys sweep** (L0
  capsule cast) from pivot to desired position resolves collision — camera pulls
  in smoothly on hit, recovers on clear (hysteresis, no pop-pop). The boom
  **never intersects geometry** — property-tested, not tuned.
- **Damped follow**: positional lag (separate horizontal/vertical damping — the
  GTA feel where the camera settles behind motion) with rotation damping tuned
  independently; hard clamp so the target never leaves a screen-space window at
  speed.
- **Auto-recenter**: after look-input idle timeout, yaw eases behind movement
  direction (rate scaled by speed — strong while sprinting, off while strafing
  slowly/standing); user look input always wins instantly.
- **Zoom tiers**: near/mid/far distances on a toggle action (GTA-style cycling),
  each tier = its own offset/damping row in the preset.
- **Aim mode**: over-the-shoulder tighten (shorter boom, lateral offset, FOV
  nudge, reticle on) with smooth in/out — the bridge toward 29's ADS when a game
  mixes 3P movement + 1P aiming.
- **Player fade**: when the boom is forced very short, the player mesh
  cross-fades (dither) instead of clipping — threshold in the preset.
- Pitch limits, over-the-top flip prevention, water/ceiling edge cases handled
  in the rig, not by every game.

### View switching

1P ↔ 3P on an action, runtime, free by construction — topic 29's
one-skeleton/one-pose + full-sync guarantees mean the toggle changes _camera and
model visibility flags only_. Games may lock either mode.

## Input surface (19)

Reserved-but-rebindable standard set: `move`, `look`, `jump`, `sprint`,
`crouch`, `aim`, `view_toggle`, `interact` — declared by the kit, extended
freely by games. On-screen/touch and gamepad bindings ship in the default maps
(the kit is device-agnostic because 19 is).

## Optionality contract (the Source 2 promise)

- **Adopt**: register the kit's systems + presets — a playable character in
  minutes.
- **Tune**: everything gameplay-visible is preset data (RON, hot-reloadable) —
  most games never leave this tier.
- **Replace a piece**: keep movement, swap the camera rig (or inverse) — pieces
  communicate through ordinary components, no private channels.
- **Ignore**: don't register it; roll your own on the same public seams the kit
  uses. The kit existing costs such games nothing (it's not compiled into the
  sim unless registered).
- CI enforces the dogfood rule: `crcbl-player` builds against the module API
  surface only (same check as samples' "engine API only" rule).

## Testing (topic 12)

- Movement goldens: slope/step/jump traversal courses (fixture maps) with input
  scripts → position hashes; coyote/buffer timing tables.
- Boom property: camera position never inside geometry across randomized
  orbit/terrain fuzz (sweep-resolved by construction, asserted anyway).
- Recenter/damping behavior tables (input-idle scenarios → expected yaw
  convergence curves within tolerance).
- Prediction integration: kit movement passes the 26 zero-divergence property
  under clean network (the kit is the canonical predicted system).
- View-toggle parity: rides 29's pose/cosmetic/audio parity suites.

## Delivery (wave 1 — lands with/inside puppet)

1. Movement controller + presets (responsive + milsim) — puppet adopts it (its
   hand-rolled controller is replaced; puppet becomes the kit's fixture).
2. 3P GTA rig (boom/damping/recenter/zoom) + player fade.
3. 1P binding + view toggle (29 dependency; full polish lands FPS-era).
4. Aim mode + on-screen/touch defaults.
5. Optionality CI check + `crcbl new` template wiring.

## Risks

- **Feel is subjective**: presets + hot reload + the puppet playground make
  tuning cheap; the defaults chase "good GTA-like", not universal.
- **Kit privilege creep**: the dogfood CI check is the guard — the kit breaking
  without a public API is the signal an engine seam is missing.
- **Camera edge-case whack-a-mole** (corners, ceilings, vehicles-later): the
  boom property test + fixture courses grow per bug; edge cases become fixtures,
  not folklore.
