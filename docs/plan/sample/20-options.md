# Sample 20 — options (S4E, gates P10)

Settings acceptance test. A settings screen with nothing behind it: every video,
graphics and audio setting the engine defines, changed live, written to disk,
and still there after a restart — on the desktop and in a browser tab.

**This is the sample that closes a round trip nothing else closes.** The
settings machinery is built: `crates/crcbl-store/src/settings.rs` is a layered
TOML stack with `get`, `set` and `save`, and `SettingsStack::platform` already
picks the platform config directory on native and OPFS on wasm. What has never
happened is an _application_ writing a setting. The only writer in the workspace
is `crates/crcbl-cli/src/settings_cmd.rs` — a command line. So the layer that
stores a player's choices has been exercised by everything except a player.

## Proves

- **A setting survives a restart, on both platforms.** Change it, quit,
  relaunch, and it is still set — natively through the platform config
  directory, and in the browser through OPFS. The web half is the one that is
  easy to claim and easy to get wrong, since a browser tab has no filesystem and
  the failure mode is silent.
- **The full catalogue is reachable from a screen**, not from a text editor:
  display mode, resolution, render scale, present mode, frame cap, the graphics
  quality tiers, and the audio bus volumes.
- **A quality preset and its custom escape hatch behave.** Picking a preset sets
  every key it owns; touching any single key drops the preset to custom. That is
  how every settings screen a player has used behaves, and getting it wrong is
  how a screen silently discards a player's tuning.
- **Audio buses are independently audible.** Music down and effects up is a
  thing this sample lets you do and hear, which is the check that a bus is a
  real gain stage rather than a key nothing reads.
- **The clamp order holds from the outside.** `[engine.video]` may only clamp
  downward and an absent key clamps nothing —
  [39-capabilities.md](../39-capabilities.md)'s rule, and a settings screen is
  the first thing that can violate it by writing a key that raises quality above
  what the device offers. The sample shows the requested value and the resolved
  value separately, so a clamp is visible rather than confusing.
- **A setting with no implementation is honestly labelled.** Most of the
  catalogue has no reader today. A screen that offers a control which does
  nothing is worse than one that says so, and this sample says so.

## Scope

- A settings screen built from the widget set — this is a consumer for the
  styled widgets that `apps/hud` is the gallery for, and the first one outside
  that gallery.
- Enough of a scene behind the screen to make a video setting visible: something
  that shows a resolution change, a render-scale change and a quality tier
  change. It does not need to be a good scene; it needs to be a changed one.
- Enough audio to make the buses audible: a music loop, a repeating effect, a UI
  click on the widgets themselves. Three buses with obviously different content
  is the minimum that makes a mixer legible.
- A reset-to-defaults control, and a view of the settings file as it stands —
  `SettingsStack::dump` already produces one.
- Pages web demo, which is the point of half this sample.

## Non-goals (hard cap)

Gameplay. Input rebinding — that is `docs/plan/19-input.md`'s catalogue and its
own screen, and a keybinding UI is a larger problem than everything else here
put together. Accessibility settings beyond what the catalogue defines. A
settings migration format beyond what topic 14 already specifies. Per-monitor or
per-adapter profiles, which topic 15 refuses with a reason.

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the
subject is a settings screen, on the same ground `apps/hud` is exempt. **Exempt
from rules 2 and 10**: no game state, no `World`, no `GameModule` — the settings
are the content.

## Status: unbuilt, and the gap is narrower than it looks

Nothing exists, but very little of what this sample needs is machinery. Verified
against the tree:

- **Storage and the layered stack are built**, native and wasm, read and write.
- **`crates/crcbl/src/settings.rs` reads four boolean keys** and maps each to a
  `RenderEffects` bit. Every other key in the catalogue has a home in the TOML
  convention and no reader.
- **`crates/crcbl-audio/src/mixer.rs` has a per-voice gain and one overall gain,
  and no bus concept at all** — so a player cannot turn music down without
  turning everything down.
- **There is no settings UI anywhere in the workspace.**
- **OPFS is the only browser storage backend**; `crates/crcbl-store/src/lib.rs`
  records that an IndexedDB fallback is still to come. Where no store is
  installed, settings are not persisted and a log line says so — which this
  sample must surface to the player rather than swallow, because a settings
  screen that silently forgets is the worst version of this bug.

## Milestones

1. **The screen, the audio buses and the round trip.** Bus volumes are the
   cheapest setting to make real — they need no renderer change — so they are
   what proves save, load and restart first.
2. **The video half**: display mode, resolution, present mode, frame cap, and
   the requested-versus-resolved display of the clamp.
3. **The graphics half**: the quality tiers over the technique ladders, the
   preset and its custom escape hatch.
4. **The browser half held to the same bar**, including the no-store case.

## Exit criteria

- Every key in the settings catalogue appears on the screen, and any key with no
  reader is labelled as such.
- A changed setting survives a restart on desktop and in a browser tab.
- Music and effects volumes are independently audible.
- Preset selection and the drop-to-custom rule both behave.
- Requested and resolved values are shown separately wherever they differ.
- A headless run can set every key and dump the resulting file, so the round
  trip is testable without a human.
- Web demo deployed, including its behaviour when no OPFS store is installed.
