# Sample 04 — hud

Pure UI demo: a game-style HUD plus a widget gallery, built entirely with the
CSS-subset UI system (stage 7). No gameplay — the UI system _is_ the game here.
Doubles as the UI system's living fixture: it starts as a skeleton the moment
the UI core lands (P4) and grows with every widget/property until it completes
at P10.

This sample also carries the wider claim: **the entire engine's own UI — debug
overlay, editor chrome, every sample HUD — is built with this same system and
styled by stylesheets.** hud is where that workflow is proven and demonstrated.

## Proves

- **The web-workflow claim, end to end**: a full HUD (health/mana bars, minimap
  frame, ability row with cooldown states, wave banner, damage ticker) built as
  blocks/spans + a `.css` file. Restyle live — edit the stylesheet, watch the
  running app change, no recompile.
- **Theming = stylesheet swap**: the demo ships ≥2 complete themes (e.g. "clean
  esport" and "fantasy") selected at runtime — same tree, different css. This is
  the pattern games use to skin engine widgets.
- **Layout engine coverage**: the gallery page exercises every supported
  flex/box property with visible expected-vs-actual fixture panels — the
  human-eyeball complement to the automated layout property tests (topic 12).
- **Widget gallery**: every widget in its states (hover/active/focus/ disabled)
  on one scrollable page — the visual regression surface (golden frames per
  theme) and the widget documentation, simultaneously.
- **UI inspector dogfood**: built _with_ the inspector; the demo's help overlay
  teaches inspector usage (hover → boxes, matched rules, computed values).
- **Pseudo-state styling**: `:hover`/`:active`/`:focus`/`:disabled` visibly
  driven by the stylesheet, not widget code.

## Scope

- Two pages: **HUD demo** (fake game data animating the widgets — timers, damage
  numbers, cooldowns driven by a scripted loop) and **gallery**.
- Theme switcher, live `.css` hot-reload showcase, UI inspector toggle.
- Fake data only — no server simulation beyond a trivial ticker (still runs the
  standard client/server shape; the ticker is a server system, per sample rule
  2).
- Web build on the Pages site — doubles as the "try the UI system in your
  browser" landing demo.

## Non-goals (hard cap)

No gameplay, no scene rendering behind the HUD beyond a static backdrop, no
widget additions that no other consumer needs (the gallery shows what exists; it
doesn't drive speculative widgets — that's the reverse of the rule).

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): this
sample's subject _is_ the widget system, and a sprite sheet in front of it would
be showing something other than what it exists to show. The one place art does
belong is skinned buttons — `Button::with_skin` takes nine-slice insets and the
gallery is where every state of a skinned widget is on one screen at once, so
the skin sheets themselves are `.crpix`. Rule 4's debug panel still applies, and
here it is the dogfood case: the panel is built out of these widgets.

## Milestones

1. **P4 skeleton** ✅ **built**: HUD page with the slice-1 primitives (blocks,
   spans, text, bars) — becomes the UI system's dev fixture immediately.
2. Grows a widget/property at a time alongside P10 work — every new widget lands
   with its gallery entry + golden frame in the same PR.
3. **P10 complete**: both pages, two themes, inspector, hot-reload demo; golden
   frames per theme in CI.

### What milestone 1 actually landed

`apps/hud`, modelled on `apps/flappy` minus its art, audio and browser halves —
`main.rs`, `args.rs`, `game.rs`, `page.rs`, `menu.rs`, `gpu.rs`, `app.rs`.

- **The page** (`page.rs`) is the whole of what this sample draws, and every
  element on it is a `DrawList::rect`, a `DrawList::rect_outline` or a
  `DrawList::text`: a vitals panel with health and mana **bars** (a track rect
  and a fill rect that spans the fraction of the pool that is left), a **wave
  banner** shown for `BANNER_TICKS` after each turn-over, an **ability row** of
  four slots each showing ready-or-cooling with a sweep that retreats as the
  cooldown runs down, and a **damage ticker** whose numbers climb their lane and
  fade. Laid out against the acquired extent, not against a fixed 960×720.
- **The ticker** (`game.rs`) is a `GameModule` the authoritative server owns,
  stepped on the fixed timestep with a client on the other end of an
  `InMemoryTransport` — sample rule 2, with no exemption taken. Integer counters
  throughout, with `--seed` indexing `crcbl::core::rand::hash_u64` for the
  damage rolls, so a run replays bit-identically.
- **Rule 4's debug panel** is the dogfood case and hud contributes two sections:
  `hud` (the ticker's counters) and `page` (how many rects, outlines and text
  spans the frame's page emitted, tallied off the draw list itself).
- **Rule 11 exemption taken**: no `.crpix`, no `build.rs`, no sprite pass. The
  frame is a `backdrop` clear, the menu pass and `ui-composite`, which makes it
  the only sample whose UI pass has nothing in front of it in CI's `vk-e2e` job.
- **No game input.** Every key is the loop's — `ESC`, `F3`, `F11` — because the
  page is driven end to end by the ticker. The one menu is the pause panel, and
  every button on it is one the loop already owns, so `MenuAction` is
  `Infallible`.

Not built, and each waiting on the styling system rather than on this sample:
the CSS subset and its stylesheets, the theme switcher, the gallery page, the UI
inspector, hot reload, and per-theme golden frames. The wasm front end and the
Pages demo are deferred with them — see `docs/backlog.md`.

## Exit criteria

- Both themes render pixel-stable (golden frames per theme green in CI).
- The live-restyle demo works headless too: `crcbl screenshot` before/after a
  css edit shows the change (CI-verifiable web-workflow).
- A new widget cannot merge without a gallery entry + golden frame (enforced by
  review checklist; the gallery is the widget registry).
- Full gallery traversable by pad/arrows/WASD alone (focus-path e2e green) —
  pointer never required.
- Published on the Pages site; loads fast (UI-only bundle is the smallest wasm
  artifact — measure and record it).
