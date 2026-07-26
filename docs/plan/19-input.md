# Topic 19 — Input: Device-Agnostic Action System

Godot/Unity-style action mapping, engine-wide: gameplay and UI code consume
**actions**, never devices. Keyboard, mouse, gamepad, and on-screen (touch)
controls are interchangeable binding sources behind one config layer — swap
devices mid-session, rebind everything, same game code.

## Layered design

```
Shell raw events (topic 15)          — scancodes, buttons, axes, touches; per-device id + timestamps
        ↓
Device layer (crcbl-input)           — device registry (kind, id, connect/disconnect), axis
                                       normalization, deadzone/response curves per device kind
        ↓
Action layer                         — ActionMap: bindings → actions, patterns, contexts
        ↓
Consumers                            — gameplay (via P2 input pipeline → server), UI, editor
```

The **server sees actions, not keys**: the client resolves bindings locally and
replicates action state (`Move: vec2`, `Jump: pressed`) — device agnosticism is
structural, replays/bots inject actions, and rebinding never touches netcode.

## Actions

Declared by the game (module `init` / static registration), typed:

- `Button` — pressed/released/held state + this-tick edges.
- `Axis1` — analog −1..1 or 0..1 (triggers).
- `Axis2` — stick/WASD-composite/touch-stick vector (normalized, magnitude
  preserved for analog).

**Patterns** attach to button actions where the game wants them, evaluated in
the action layer (one implementation, every device): `press`, `release`,
`hold(duration)` (fires after held N ms — the "hold pattern"), `tap`
(press+release under N ms), `double-tap(window)`, `repeat(rate)`.

**Contexts** (action sets): `gameplay`, `ui`, `editor`, game-defined (e.g.
`vehicle`). Stack-based push/pop; an active context consumes its bound inputs,
unbound inputs fall through. This replaces ad-hoc "UI ate the input" rules from
stage 7 with one declarative system.

## Bindings

Per action, per **device class** — all optional, all simultaneous:

```ron
// game defaults (asset, RON); user overrides merge from profile (topic 14)
(
  action: "jump",
  kind: Button,
  bindings: (
    keyboard: [Key(Space)],
    mouse:    [],
    gamepad:  [Button(South)],            // A/Cross — positional, not labeled
    touch:    [Virtual("btn_jump")],      // on-screen control id
  ),
  patterns: [Hold(400, "jump_charge")],   // held ≥400ms emits jump_charge
)
(
  action: "move",
  kind: Axis2,
  bindings: (
    keyboard: [Composite(w: Key(W), s: Key(S), a: Key(A), d: Key(D))],
    gamepad:  [Stick(Left, deadzone: Radial(0.15))],
    touch:    [Virtual("stick_move")],
  ),
)
```

- Multiple bindings per device class allowed (Space _and_ pad-South both =
  jump); last-active-device tracked (drives UI glyph hints: show Ⓐ vs `Space`
  automatically).
- Gamepad buttons are positional (South/East/North/West) with per-vendor glyph
  mapping — the Nintendo-swap problem handled in presentation, not bindings.
- **User rebinds** live in the profile (topic 14, RON) as diffs over game
  defaults; the P10 settings screen edits them (listen-for-input rebind flow
  provided by the engine); `crcbl input` CLI inspects/edits.

## Device backends (zero 3rd-party rule, topic 15 discipline)

| Device                 | Backend                                                                                                                                                                     | Lands                               |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| Keyboard/mouse         | already in shell backends                                                                                                                                                   | P0                                  |
| Gamepad Linux          | evdev directly (`/dev/input/event*`, ioctl caps, force-feedback later)                                                                                                      | P10                                 |
| Gamepad Windows        | XInput (hand FFI; GameInput later if needed)                                                                                                                                | P14                                 |
| Gamepad macOS          | GameController framework (objc FFI)                                                                                                                                         | P14                                 |
| Gamepad Web            | Gamepad API via the JS shim                                                                                                                                                 | P10-ish (cheap, with wasm demos)    |
| **On-screen controls** | `crcbl-ui` widgets (stick, buttons, dpad) emitting virtual-device events — a _bindable device_ like any other; layout = a UI stylesheet, so games reskin/reposition via css | post-MVP (with mobile-web interest) |

Hotplug: connect/disconnect events; local multiplayer device-to-player
assignment is post-MVP but the device-id plumbing supports it from day one.

## Debug + testing

- Input inspector panel: live devices, raw values, resolved action states,
  active context stack, last-active device.
- Determinism: recorded input scripts (stage 4) record **actions** — device
  playback not needed; binding-resolution unit tests per pattern
  (tap/hold/double-tap timing tables); composite/deadzone math unit tests; e2e
  via HeadlessShell scripted raw events → expected action stream.

## Delivery

| Slice                                                           | Phase                                           |
| --------------------------------------------------------------- | ----------------------------------------------- |
| Action layer (actions, patterns, contexts, RON maps) + kb/mouse | P2 (replaces the raw-input pipeline plan there) |
| Profile rebind storage + glyph hints                            | P4                                              |
| Rebind UI in settings screen; input inspector                   | P10                                             |
| Gamepad: evdev (Linux) + Web Gamepad API                        | P10                                             |
| Gamepad: XInput / GameController                                | P14                                             |
| On-screen touch controls (UI-widget virtual device)             | post-MVP                                        |
| Local-multiplayer device assignment, haptics                    | post-MVP                                        |

Samples: breakout onward consume actions from S1 (paddle = `move` Axis1 —
proving the layer before gamepads even exist); puppet (09) is the full showcase:
swap kb/mouse ↔ gamepad ↔ on-screen pad live, same character.

## Risks

- **Pattern timing edge cases** (hold-vs-tap races, double-tap windows): pattern
  evaluator is a pure function over timestamped edges — unit-test tables, not
  playtesting, define the semantics.
- **evdev permission/quirk zoo**: scoped to standard gamepads first
  (SDL-database-style per-device mapping is a rabbit hole — support the common
  controllers, map table grows by demand, no 3rd-party DB import).
- **Context-stack abuse** (input eaten mysteriously): inspector shows the full
  resolution path per input — debuggability designed in.
