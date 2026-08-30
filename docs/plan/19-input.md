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

**What is built is the layer under that file, and its shape differs.** There is
no RON binding asset and nothing parses one yet; a game declares its actions in
code, through `crcbl_input::ActionDecl { name, kind, bindings }`. The three
`ActionKind`s are as above, but `bindings` is one flat `Vec<Binding>` rather
than a per-device-class record — a `Binding::Key`, a `Binding::MouseButton` and
a `Binding::Virtual` sit side by side on the same action and nothing downstream
can tell which of them spoke, which is the device-agnosticism this topic is for
arriving a level lower than the file. `Binding` also carries members this sketch
does not: `PointerPosition { axis }` for where the pointer _is_, normalised
rather than in pixels, and `KeyAxis { negative, positive }` for a
keyboard-driven `Axis1`, since a single key can only ever push an axis positive.
The composite is `Binding::Wasd { up, down, left, right }` and is normalised, so
a diagonal is a unit vector. **Patterns and contexts are not built**: an action
carries no pattern list, and a button reports `ButtonState::Held { duration }`
for the game to interpret rather than firing a named `hold` — there is no `tap`,
`double-tap` or `repeat` evaluator, and no context stack. Those parts of this
document are still the plan.

## Device backends (zero 3rd-party rule, topic 15 discipline)

| Device                 | Backend                                                                                                                                                                                       | Lands                            |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| Keyboard/mouse         | already in shell backends                                                                                                                                                                     | P0                               |
| Gamepad Linux          | evdev directly (`/dev/input/event*`, ioctl caps, force-feedback later)                                                                                                                        | P10                              |
| Gamepad Windows        | XInput (hand FFI; GameInput later if needed)                                                                                                                                                  | P14                              |
| Gamepad macOS          | GameController framework (objc FFI)                                                                                                                                                           | P14                              |
| Gamepad Web            | Gamepad API via the JS shim                                                                                                                                                                   | P10-ish (cheap, with wasm demos) |
| **On-screen controls** | `crcbl_ui::touch`'s `TouchStick` and `TouchButton` hit-test raw contacts and report through `ActionMap::virtual_stick` / `virtual_button`, so a `Binding::Virtual` is a device like any other | shipped                          |

**The stylesheet the table used to promise is not what shipped, and the reason
is worth keeping.** A control's palette is one `crcbl_ui::touch::CONTROL_STYLE`
constant — a `Style`, not a document, and there is no stylesheet system in the
workspace for it to be a rule in. It is a constant on purpose: a control is
drawn over the game's field, so it is dark and translucent, and holding that in
one place is what stops a stick and a button on the same screen — or the same
button in two samples — drifting into two palettes. Geometry is the widget's own
(`TouchStick` is _floating_: it appears where the finger lands), not a
stylesheet's. Reskinning per game is therefore not available and is not
currently planned; it needs the CSS/DOM work in
[07-ui-debug.md](07-ui-debug.md), which is unbuilt.

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

| Slice                                                                                                                                                                                                                                                                                                                                      | Phase                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------- |
| Action layer + kb/mouse — **patterns, contexts and the RON binding asset are not built**, per the section above: an action carries no pattern list, there is no context stack, and nothing parses a binding file.                                                                                                                          | P2 (replaces the raw-input pipeline plan there) |
| Profile rebind storage + glyph hints — **neither built**, checked 2026-08-23: `crcbl-store` has no profile or binding type (its only cross-session helper is `record.rs`, one number for the samples' high scores) and `crcbl-input` contains no glyph anything. `ActionMap::rebind` exists and is in-memory only — nothing serialises it. | unbuilt                                         |
| Rebind UI in settings screen; input inspector                                                                                                                                                                                                                                                                                              | P10                                             |
| Gamepad: evdev (Linux) + Web Gamepad API                                                                                                                                                                                                                                                                                                   | P10                                             |
| Gamepad: XInput / GameController                                                                                                                                                                                                                                                                                                           | P14                                             |
| Local-multiplayer device assignment, haptics                                                                                                                                                                                                                                                                                               | post-MVP                                        |

Samples: breakout onward consume actions from S1 — proving the layer before
gamepads even exist. What breakout actually declares is `left` and `right` as
`Button`s, `launch` and `restart` beside them, and an `aim` `Axis1` bound to
`Binding::PointerPosition`, which is what lets a phone play it with the primary
contact and no on-screen control at all. The `move` `Axis2` is horde's, where a
`Binding::Wasd` composite and a `Binding::Virtual` stick drive the same action.
puppet (09) is the full showcase: swap kb/mouse ↔ gamepad ↔ on-screen pad live,
same character.

## Risks

- **Pattern timing edge cases** (hold-vs-tap races, double-tap windows): pattern
  evaluator is a pure function over timestamped edges — unit-test tables, not
  playtesting, define the semantics.
- **evdev permission/quirk zoo**: scoped to standard gamepads first
  (SDL-database-style per-device mapping is a rabbit hole — support the common
  controllers, map table grows by demand, no 3rd-party DB import).
- **Context-stack abuse** (input eaten mysteriously): inspector shows the full
  resolution path per input — debuggability designed in.

## Correction (2026-08-09)

**`DeviceId` names a device _kind_, not a device, on every backend that has
one.** Win32, X11 and AppKit all report a constant per family. The layered
design above declares "per-device id" at the shell boundary and states that "the
device-id plumbing supports [local multiplayer] from day one" — it does not,
today, on any platform, and the device registry cannot tell two keyboards apart.

Per-backend routes exist and none is free: Windows is best placed
(`RAWINPUTHEADER::hDevice` identifies the physical device on every `WM_INPUT`,
but raw input would have to become the source of button and wheel events too,
and a handle needs a table and a hotplug story); macOS and Linux both end at
IOKit and evdev respectively, which is the same slice as unaccelerated raw
motion.

Consequence to state plainly: **local-multiplayer device assignment is
blocked**, not merely unscheduled, and any test asserting two devices are
distinguishable would pass vacuously today. `docs/backlog.md` carries the
per-backend detail.
