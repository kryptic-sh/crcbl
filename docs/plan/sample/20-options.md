# Sample 20 — options (S4E, gates P10)

Settings acceptance test. A settings screen with nothing behind it: every video,
graphics and audio setting the engine defines, changed live, written to disk,
and still there after a restart — on the desktop and in a browser tab.

**This is the sample that closes a round trip nothing else closes.** The
settings machinery is built: `crates/crcbl-store/src/settings.rs` is a layered
TOML stack with `get`, `set` and `save`, and `SettingsStack::platform` already
picks the platform config directory on native and OPFS on wasm. What had never
happened is an _application_ writing a setting: the only writer in the workspace
was `crates/crcbl-cli/src/settings_cmd.rs`, a command line. So the layer that
stores a player's choices had been exercised by everything except a player.
`apps/options` is now the second writer, and the first one a player can reach.

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

## Status: the audio half is built, the video half is not

`apps/options` exists and edits the audio buses; the rest of the catalogue is
still only reachable from a text editor. Very little of what is left is
machinery. Verified against the tree on 2026-08-28:

- **Storage and the layered stack are built**, native and wasm, read and write.
- **A setting is writable from an application**, as of 2026-08-28.
  `SettingsStack::with_platform_storage` lends the storage `platform` used to
  resolve and drop, `save_platform` writes the user layer back to it, and
  `crcbl::settings` has `set_video`, `set_video_effects`, `set_render_scale`,
  `set_anisotropic_filtering` and `set_audio_gain` beside its readers, with
  `SettingsSource::open` and `SettingsSource::save` as the pair a screen holds.
  The paragraph above this section still says the only writer is a command line;
  that was true when it was written and is not now.
- **`crates/crcbl/src/settings.rs` reads every `RenderEffects` boolean of
  `VIDEO_KEYS`, `render_scale`, `anisotropic_filtering`, `frame_limit` and every
  `[engine.audio]` bus volume.** `frame_limit` is the one the engine's loop
  applies for a game, through `GameGpu::video`; the rest are handed over and
  applied by the caller. `crcbl::settings::catalogue` is the enumeration — every
  key the engine defines, each marked `Read` or `Named` — and
  `crcbl settings list` marks each line of a player's file with it, so a key
  under `engine.` that the engine does not define is reported rather than
  silently written.
- **`crates/crcbl-audio/src/mixer.rs` has buses.** `Bus::ALL` is master, music,
  sfx, ui, voice and ambience; `Mixer::set_bus_gain` and `bus_gain` are the gain
  stage, `Bus::settings_key` is the spelling, and
  `SettingsSource::apply_audio_gains` is what four samples already call at
  start-up. The claim below this line — that there is neither a bus nor a master
  — was true of an earlier tree.
- **There is a settings UI, and it edits the audio half.** `apps/options` is
  built: `menu::menus` lays out a fader per bus over the styled widget set,
  `app::Screen` reconciles the faders against the keys every frame, and `SAVE`
  writes the user layer through `SettingsSource::save`. What the workspace still
  has no screen for is the video and graphics halves — milestones 2 and 3 —
  which is now the largest single thing this sample owes.
- **The faders reach a gain stage**, as of 2026-08-28:
  `apps/options/src/audio.rs` banks a looping tone on `Bus::Music`, a noise tick
  once a `TICK_PERIOD` on `Bus::Sfx` and a click on `Bus::Ui`, and `Screen::set`
  — the one place a gain changes — moves `Mixer::set_bus_gain` in the same call
  that writes the key. `Bus::Voice` and `Bus::Ambience` have no content, which
  `Audio::sounds` answers and `menu::fader_hint` writes on the row as
  `(silent)`.

  What is asserted is the mixer, not a room:
  `pulling_the_music_bus_down_makes_the_mix_quieter` measures a block of the mix
  at two gains, which is as far as a test can take "audible". **Nobody has
  listened to it on hardware.** That is the gap between this and the exit
  criterion below.

- **The frame cap is on the screen**, as of 2026-08-28, and is the first
  `[engine.video]` key to reach one. `menu::FRAME_CAPS` is the ladder it steps
  through and `menu::is_above` is the ordering, asked of
  `FrameLimit::clamped_to` rather than of the rates. It is also the first
  setting here that does **not** apply as it moves — the loop takes its ceiling
  when it is built — so the row carries `menu::NEXT_START_MARK` until a run has
  started under it. That is the honest half of requested-versus-resolved; the
  other half, showing the game's own `--fps` and the device's clamp beside the
  file's value, is still open and `docs/backlog.md` carries it.
- **The anisotropy is on the screen**, as of 2026-08-29, on the cap's pattern:
  `menu::ANISOTROPIES` is the ladder — `1`, `2`, `4`, `8`, `16`, the powers of
  two hardware steps in — `menu::next_anisotropy` lifts a hand-written value
  between rungs to the rung above, and `RESET` puts it back to
  `DEFAULT_ANISOTROPY` rather than to the bottom, since that is what an absent
  key means. It wears `menu::NEXT_START_MARK` too: this sample draws no page, so
  the key reaches `ForwardRenderer::set_anisotropy` only where a renderer opens
  over it, which today is `apps/viewer`.
- **The render scale is on the screen**, as of 2026-08-29, and it is a groove
  rather than a ladder because the key is a continuum: `menu::scale_at` and
  `menu::scale_handle_at` map the handle linearly over `MIN_RENDER_SCALE..=1.0`,
  the range the renderer clamps to, and the screen reconciles it both ways on
  the faders' rule — placed from the file before it is read as a drag, following
  `RESET` back to the whole extent. It wears `menu::NEXT_START_MARK` for the
  anisotropy's reason, and drags silently: the detent click belongs to a bus.
- **The effect switches are on the screen**, as of 2026-08-29, one row per entry
  of `crcbl::settings::VIDEO_KEYS` — derived from the table rather than listed
  again, so a key added there gets a row here without a second spelling.
  `menu::effect_id` and `menu::effect_of` are the pair `fader_id` and `bus_of`
  are for a bus; a press flips one bit and `Screen::set_effects` writes every
  key, which is what `set_video_effects` does and what leaves the file saying
  what was chosen for each switch. `RESET` allows everything, since that is what
  an absent section means. The mark is per switch: `on (next start)` on a row
  the run came up with off.
- **The screen runs in a browser**, as of 2026-08-28: `apps/options/src/web.rs`
  carries the `__crcbl_options_*` ABI, `web/demos/options/` and
  `web/pages/options.html` are the page, and both browser-gate jobs run it. Its
  `EXPECTATIONS` row is the only one in `web/tools/browser-e2e.mjs` marked
  `still` — a settings screen has nothing to animate, so group D asks the loop's
  own frame counter for the claim the frame hash makes everywhere else. Its
  `settings` block is the round trip itself, and it is the only place in the
  workspace where a saved setting is read back after a reload.
- **OPFS is the only browser storage backend**; `crates/crcbl-store/src/lib.rs`
  records that an IndexedDB fallback is still to come. Where no store is
  installed, settings are not persisted and a log line says so — which this
  sample must surface to the player rather than swallow, because a settings
  screen that silently forgets is the worst version of this bug.

## Milestones

1. **The screen, the audio buses and the round trip.** Bus volumes are the
   cheapest setting to make real — they need no renderer change — so they are
   what proves save, load and restart first. **The screen, the round trip and
   the web build are done**, in `apps/options`: six faders, a `SAVE` that writes
   the user layer and reports where it went, a `RESET` that puts every bus back
   to unity, a start that opens the player's own file and places the faders from
   it, and a page at `/demos/options/` built from the same source and run by the
   browser gate on every push — which drives the round trip through the
   keyboard, finds `settings.toml` in the OPFS root, reloads onto the saved gain
   and then wipes the store and requires unity back. What is left of this
   milestone is **audible in a browser and on hardware**: the cues are there and
   the mixer is measured, but the browser gate asserts nothing about audio for
   any demo, and no run of this sample has been listened to. `docs/backlog.md`
   carries both, along with the one browser case still unreached — a page with
   no store installed, which is `SaveState::Nowhere`.
2. **The video half**: display mode, resolution, present mode, frame cap, and
   the requested-versus-resolved display of the clamp. **The frame cap is
   done**, in `apps/options`'s `FRAME CAP` row — the cheapest of the four,
   because `crcbl::settings::frame_limit` is the only one of them with a reader
   today — and the `ANISOTROPY` and `RENDER SCALE` rows and the seven effect
   switches are the graphics half's keys on the screen, ahead of the tiers. What
   the other three need first is somewhere for a change to land: a display mode
   or a resolution is applied to a live window, not read once at start-up, so
   each of them wants a seam this sample does not have yet.
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
