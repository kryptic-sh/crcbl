# Topic 14 — Persistence (Saves, Settings, Profiles)

Engine-level persistence: save games, game settings, player profiles/local data.
First-party (`crcbl-store`), built on machinery that already exists — save games
are the replication snapshot written to disk; settings are the TOML config
layer; everything goes through an async storage seam so wasm is a first-class
citizen.

## The three kinds of persisted data

| Kind          | Content                                                | Format                                         | Owner          |
| ------------- | ------------------------------------------------------ | ---------------------------------------------- | -------------- |
| **Save game** | full server world state at a tick                      | versioned binary container (snapshot encoding) | server         |
| **Settings**  | engine + game options (video/audio/input/gameplay)     | TOML, layered                                  | client (local) |
| **Profile**   | per-player local data: high scores, unlocks, key binds | RON (engine data rule)                         | client (local) |

Format note: the RON-for-engine-data rule applies to _authored/inspectable_ data
(profiles, small saves in debug). Save games are runtime state — they use the
compact snapshot binary encoding (same encoder as replication) inside a
versioned container; `crcbl save dump` renders any save as RON for inspection
and diffing (topic 11), so debuggability is kept without paying text-format cost
on the hot path.

## Save games = snapshots (no second serialization path)

- The stage 4 `SnapshotWriter` already serializes every replicated system's
  state per tick. A save game is: container header (format version, engine
  version, scene ref + hash, server tick, playtime, optional thumbnail) + **full
  snapshot** + per-system extension blocks for state that replicates
  incrementally but persists fully.
- **Server-side operation**: saves capture authoritative state, triggered by
  server command (`Command::Save`) — which means the console, the CLI, a game UI
  button, and an autosave timer all use the same path. Single player = local
  server saves locally; dedicated server = world saves live server-side (players
  hold profiles, not world state).
- **Load = scene-load path**: tear down world, instantiate from snapshot instead
  of scene chunks (stage 6 loader and editor play-mode restore already do
  snapshot restore — same code).
- Editor play-mode snapshot, save game, and join-in-progress full snapshot are
  **one mechanism** with three triggers. Keeping them unified is the design
  constraint that prevents save-game rot.
- Versioning: header carries format + per-system versions; serde defaults absorb
  additive change; a migration seam (`fn migrate(old_ver, bytes)`) exists from
  day one but stays empty in MVP.
- Atomic writes always: write temp + fsync + rename. Corrupted-save protection
  is not optional. Keep last N autosaves (ring).

## Settings

- **Layered resolution**: engine defaults → game defaults → user settings file →
  CLI/env overrides. First-write wins upward; `settings.toml` stores only
  user-changed values (diff vs defaults — small files, upgrade-friendly).
- Namespaced: `[engine.video]`, `[engine.audio]`, `[engine.input]`, `[game.*]`
  free for the game. Typed access API with serde structs + defaults; unknown
  keys warn, never crash.
- Hot-apply where possible (volume, mouse sensitivity — immediate; vsync,
  resolution — apply-on-confirm pattern provided by the engine).
- Key binds live in profile (RON) not settings TOML — they're per-player
  structured data (action → chord maps), and games extend the action set.

## Storage seam (`StorageSource`, async like `AssetSource`)

- Native: platform dirs — config (`~/.config/<game>`), data/saves
  (`~/.local/share/<game>`), cache; Windows/macOS equivalents behind the same
  trait.
- **Wasm: OPFS** (Origin Private File System) primary, IndexedDB fallback —
  async by nature, which is why the whole persistence API is async from day one
  (same wasm-forcing-function as assets). Browser saves are real saves.
- Server deployments: configurable data dir (dedicated server flag).
- Quota/failure surfaced as results, not panics — the browser can say no.

## CLI + debug (topics 11/7)

- `crcbl save list|dump|diff|restore` — dump renders snapshot as RON; `diff`
  compares two saves system-by-system (uses the deterministic encoding — great
  for "what changed between autosaves" debugging).
- `crcbl settings get|set|list` — scriptable settings.
- Debug overlay: storage panel (paths, sizes, last save tick, autosave ring
  state).

## Testing (topic 12)

- Roundtrip property: save → load → state hash equals original at same tick
  (rides the determinism harness — this is the whole correctness story in one
  test).
- Version-skew: load fixtures from older format versions in CI (fixtures checked
  in per released version).
- Atomicity: kill-during-write test leaves previous save intact.
- Settings: layer-resolution unit table; unknown-key tolerance.
- Wasm: OPFS roundtrip in the browser e2e job.

## Delivery (interleaved — see ROADMAP)

| Slice                                                         | Roadmap phase                                           |
| ------------------------------------------------------------- | ------------------------------------------------------- |
| `StorageSource` + settings layers (engine.video/audio/input)  | P2 (with the sim core; settings needed by first sample) |
| Save/load container over snapshots + atomic writes + autosave | P2 (thin layer over the same phase's SnapshotWriter)    |
| Profiles (high scores, binds)                                 | P4 (breakout ships with a high score)                   |
| `crcbl save`/`settings` CLI, save diff                        | P2, grows                                               |
| OPFS wasm backend                                             | P5                                                      |
| Settings UI screen (engine-provided, CSS-styled, reusable)    | P10                                                     |
| towers: mid-run save/resume incl. co-op world save            | S6                                                      |

## Exit criteria (MVP)

- breakout: high score + settings persist across restarts (native + browser).
- towers: save mid-wave, quit, resume — solo and dedicated-server co-op (world
  save server-side; clients rejoin into it).
- Save→load→hash property green in CI; kill-during-write leaves prior save.
- `crcbl save dump/diff` works on any save; settings scriptable via CLI.
- Same game code path for autosave, manual save, console `save`, CLI save.

## Correction (design review, 2026-07-27)

**Save shape must match the galaxy wire model (23).** "Full server world state
at a tick" is incoherent once "full world snapshot" stops existing as a concept
beyond one sector. A save is:

```
header (versions, scene ref+hash, tick, playtime)
+ sector set (which sectors this save covers)
+ per-sector snapshots (the same encoder replication and replay use)
+ on-rails elements (orbital parameters for everything not live-simulated)
+ per-system extension blocks
```

Single-sector games (every MVP sample) produce exactly the original format, so
nothing gets more complex early — but the container is correct from P2 instead
of being restructured after saves ship.
