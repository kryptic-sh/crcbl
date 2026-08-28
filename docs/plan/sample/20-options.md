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
against the tree on 2026-08-28, and **three of these rows had gone stale in the
engine's favour** — the mixer, the reader count and the writer gap have all
moved since this document was written:

- **Storage and the layered stack are built**, native and wasm, read and write.
- **A setting is writable from an application**, as of 2026-08-28.
  `SettingsStack::with_platform_storage` lends the storage `platform` used to
  resolve and drop, `save_platform` writes the user layer back to it, and
  `crcbl::settings` has `set_video`, `set_video_effects`, `set_render_scale` and
  `set_audio_gain` beside its readers, with `SettingsSource::open` and
  `SettingsSource::save` as the pair a screen holds. The paragraph above this
  section still says the only writer is a command line; that was true when it
  was written and is not now.
- **`crates/crcbl/src/settings.rs` reads every `RenderEffects` boolean of
  `VIDEO_KEYS`, `render_scale`, `frame_limit` and every `[engine.audio]` bus
  volume.** `frame_limit` is the one the engine's loop applies for a game,
  through `GameGpu::video`; the rest are handed over and applied by the caller.
  `crcbl::settings::catalogue` is the enumeration — every key the engine
  defines, each marked `Read` or `Named` — and `crcbl settings list` marks each
  line of a player's file with it, so a key under `engine.` that the engine does
  not define is reported rather than silently written.
- **`crates/crcbl-audio/src/mixer.rs` has buses.** `Bus::ALL` is master, music,
  sfx, ui, voice and ambience; `Mixer::set_bus_gain` and `bus_gain` are the gain
  stage, `Bus::settings_key` is the spelling, and
  `SettingsSource::apply_audio_gains` is what four samples already call at
  start-up. The claim below this line — that there is neither a bus nor a master
  — was true of an earlier tree.
- **There is no settings UI anywhere in the workspace**, which is now the
  largest single thing this sample owes.
- **OPFS is the only browser storage backend**; `crates/crcbl-store/src/lib.rs`
  records that an IndexedDB fallback is still to come. Where no store is
  installed, settings are not persisted and a log line says so — which this
  sample must surface to the player rather than swallow, because a settings
  screen that silently forgets is the worst version of this bug.

## Milestones

1. **The screen, the audio buses and the round trip.** Bus volumes are the
   cheapest setting to make real — they need no renderer change — so they are
   what proves save, load and restart first. **The half under the screen is
   done**: the buses exist, the reader and the writer exist, and the round trip
   is covered by `a_saved_gain_reads_back_on_its_own_bus` in
   `crates/crcbl/src/settings.rs`. What is left of this milestone is the screen
   itself and a run that restarts.
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
  trip is testable without a human. `crcbl settings set` / `list` does this for
  any key today; what it does not do is enumerate the catalogue itself, so
  "every key" is still the operator's list rather than the engine's.
- Web demo deployed, including its behaviour when no OPFS store is installed.
