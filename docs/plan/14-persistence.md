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

> **The container has a consumer and the "no second path" claim does not**
> (2026-08-27). `apps/shard` is the first and only game to use `SaveWriter` /
> `SaveReader`, and it uses them exactly as this document's 2026-07-27
> correction says an MVP sample should — one `SectorSave` at `SectorId::ZERO`,
> nothing in `crcbl-store` changed on its behalf. But **the bytes inside that
> sector are shard's own**, a hand-rolled little-endian payload with its own
> magic and its own version, not the replication encoder's output. So the
> headline of this section is still a design and not yet a fact: the first game
> to save wrote a second serialization path, because the first one was easier to
> reach from a sample than the replication encoder was. Whoever wires a game's
> snapshot systems into `SnapshotWriter` for real should expect to convert shard
> rather than to find the ground already prepared.
>
> Two smaller things shard's save module records that this document should
> honour: it puts saves in the **data** directory while
> `record::Backing::platform` answers with the **config** directory (a high
> score belongs there, a save does not), and `docs/backlog.md` marks a second
> consumer of that rule as the moment to hoist it into the engine.

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

  **Neither half of that is in the tree yet.** `crates/crcbl-store/src/save.rs`
  has no `migrate` and nothing else in the crate does either — the seam is owed,
  not empty. And the container is narrower than the bullet above it describes:
  the header is magic `CRCBLSVE`, a `format_version`, the server tick, playtime
  and a sector count, followed by the sector entries and a real SHA-256 of
  everything before it. `SaveHeader` accordingly carries only `tick` and
  `playtime_secs`. No engine version, no scene ref or hash, no per-system
  versions, no thumbnail. What the format _does_ have is the version discipline
  those depend on — `SAVE_FORMAT_VERSION` was already bumped once, when the
  checksum field turned out to be a `DefaultHasher` digest rather than the
  SHA-256 it was documented as, which is exactly the situation a migration seam
  is for.

- Atomic writes always: write temp + fsync + rename. Corrupted-save protection
  is not optional. Keep last N autosaves (ring). **Both built**:
  `crcbl_store::write_atomic`, and `save.rs`'s `AutosaveRing`, which takes a
  capacity and a filename template and hands back the slot it wrote. The
  checksum is a real SHA-256 (`crcbl_shaders::sha256`), and `SaveReader` exposes
  `open_ignoring_checksum` beside `open` so a corrupt save can still be
  inspected rather than only refused.

  **"Always" is a native always, and that is the trap in this bullet.**
  `write_atomic` is `std::fs` all the way down: temp file opened `create_new`,
  `write_all`, `sync_all`, parent-directory fsync, rename, parent fsync again.
  The Origin Private File System has no `rename` to build that shape on, even
  through JS, so the browser path is `OpfsStorage` and it keeps a different half
  of the guarantee — torn reads prevented, **durability at return not
  provided**. A save that returned `Ok` in a browser is not yet a save that
  survives the tab closing. Do not carry a native atomicity test over to the
  wasm job and call it passed.

## Settings

- **Layered resolution**: engine defaults → game defaults → user settings file →
  CLI/env overrides. First-write wins upward; `settings.toml` stores only
  user-changed values (diff vs defaults — small files, upgrade-friendly). **Two
  of the four layers exist**: `crcbl-store`'s `settings.rs` has a
  `SettingsLayer` stack with `EngineDefaults` and `UserFile` variants, appended
  in order so a later layer wins, and a write always lands in the user file.
  There is no game-defaults layer and no CLI/env override layer — a game's
  compiled-in defaults live in the game's own binary, which is what the
  `crcbl settings` CLI's one-layer stack is honest about.
- Namespaced: `[engine.video]`, `[engine.audio]`, `[engine.input]`, `[game.*]`
  free for the game. Typed access API with serde structs + defaults; unknown
  keys warn, never crash.
- Hot-apply where possible (volume, mouse sensitivity — immediate; vsync,
  resolution — apply-on-confirm pattern provided by the engine).
- Key binds live in profile (RON) not settings TOML — they're per-player
  structured data (action → chord maps), and games extend the action set.

**Profiles are the row of this document with the least behind it** (2026-08-27).
There is no `Profile` type anywhere in `crcbl-store`, no RON, and no persisted
key binds — a game declares its actions in code and a rebind does not survive
the process. What _did_ ship under the profile heading is narrower and worth
knowing by name: `crcbl_store::record::Record`, one number kept between
sessions, native config dir or OPFS chosen by platform, with an in-memory
backing for headless runs. Its own docs say why it exists — four samples had
each written the same platform arms, encode, corrupt-file case and headless rule
for a high score, and the bodies matched line for line while the names agreed
about nothing. So "profiles" today means "one `Record` per game", and the
structured half (binds, unlocks, several fields, a format that can gain one) is
unbuilt.

## Storage seam (`StorageSource`, async like `AssetSource`)

- Native: platform dirs — config (`~/.config/<game>`), data/saves
  (`~/.local/share/<game>`), cache; Windows/macOS equivalents behind the same
  trait.
- **Wasm: OPFS** (Origin Private File System) primary, IndexedDB fallback —
  async by nature, which is why the whole persistence API is async from day one
  (same wasm-forcing-function as assets). Browser saves are real saves. **Built,
  and with two real consumers**: `crates/crcbl-store/src/web/opfs.rs` behind
  `crcbl::store::web::OpfsStorage`; `apps/breakout` keeps its high score there
  and `apps/shard` writes its whole save there, each selecting the backing by
  platform. No IndexedDB fallback exists — OPFS is the only browser backend, so
  a browser without it has no persistence rather than a degraded one. That is a
  known hole, not an oversight, and `crcbl-store`'s `web` module says so in its
  own "what is not closed" list: the wasm side is a queue of `(name, bytes)`
  records that a shim could satisfy from IndexedDB without this crate knowing,
  but no shim does, and the choice has no representation in `OpfsEnvironment`.
- Server deployments: configurable data dir (dedicated server flag).
- Quota/failure surfaced as results, not panics — the browser can say no. **Half
  built**: failures do come back as `StorageError` rather than panicking, but
  `navigator.storage.estimate()` is not surfaced anywhere, so the engine learns
  about a full disk from a failed acknowledgement after the fact and cannot warn
  before a save. A quota UI has nothing to read.

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

| Slice                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Roadmap phase                                           |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `StorageSource` + settings layers (engine.video/audio/input)                                                                                                                                                                                                                                                                                                                                                                                                                                  | P2 (with the sim core; settings needed by first sample) |
| Save/load container over snapshots + atomic writes + autosave — **built**: `crates/crcbl-store/src/save.rs` (`SaveWriter`, `SaveReader`, `AutosaveRing`, SHA-256 checksum, `write_atomic`). Not built over `SnapshotWriter` in practice — see the note under the section heading.                                                                                                                                                                                                             | P2 (thin layer over the same phase's SnapshotWriter)    |
| Profiles (high scores, binds) — **the high-score half built as `crcbl_store::record::Record`**; no `Profile` type, no RON, no persisted binds. See the profiles note above.                                                                                                                                                                                                                                                                                                                   | P4 (breakout ships with a high score)                   |
| `crcbl settings get\|set\|list` CLI — **built**: `crates/crcbl-cli/src/settings_cmd.rs`, wiring `crates/crcbl-store/src/settings.rs` to a terminal. It reads and writes the player's `settings.toml` under the platform config directory; `--app`, or the package name of the project in the current directory, names the game. Its stack has one layer — the player's file — because a game's compiled-in defaults live in the game's own binary and this CLI is not it, and `list` says so. | built                                                   |
| `crcbl save` CLI, save diff — **not built**: the verb is not parsed and `crcbl-cli` has no module for it. `crcbl-store`'s `save.rs` exists and nothing in the CLI reaches it. Blocked rather than merely owed: `save dump` is specified above to render a snapshot as RON, this tree has no RON reader or writer, and adopting one is an open decision in `docs/backlog.md`. Said P2 until 2026-08-23; the exit criteria below still assume it.                                               | unbuilt                                                 |
| OPFS wasm backend — **built**: `crates/crcbl-store/src/web/opfs.rs`, consumed by `apps/breakout` and `apps/shard`                                                                                                                                                                                                                                                                                                                                                                             | P5                                                      |
| Settings UI screen (engine-provided, CSS-styled, reusable)                                                                                                                                                                                                                                                                                                                                                                                                                                    | P10                                                     |
| towers: mid-run save/resume incl. co-op world save                                                                                                                                                                                                                                                                                                                                                                                                                                            | S6                                                      |

## Exit criteria (MVP)

- breakout: high score + settings persist across restarts (native + browser).
  **The high-score half is met** — `apps/breakout`'s `high_score.rs` opens a
  `crcbl::store::record::Record`, config directory natively and OPFS in the
  browser, in-memory for a headless run.
- towers: save mid-wave, quit, resume — solo and dedicated-server co-op (world
  save server-side; clients rejoin into it). **No towers app exists**, so this
  criterion cannot be met yet. The solo half of its shape _is_ proven elsewhere:
  `apps/shard` writes a save through `SaveWriter`/`SaveReader` and reloads from
  it, native and browser. The co-op half needs both a network transport and a
  towers, and has neither.
- Save→load→hash property green in CI; kill-during-write leaves prior save.
- `crcbl save dump/diff` works on any save; settings scriptable via CLI.
- Same game code path for autosave, manual save, console `save`, CLI save.
  **Only two of those four triggers exist**: a game calling `SaveWriter`
  directly, and `AutosaveRing`. There is no console and no `crcbl save`, so the
  claim that all four share one path is untested rather than kept — nothing has
  yet tried to reach a save from outside the game process.

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

> **The sector half landed; the rest of the header did not, 2026-08-15.**
> `save.rs`'s container is a sector count and a `SectorEntry` per sector, each
> keyed by a `[i64; 3]` sector id and holding the same snapshot bytes
> replication uses — so the shape this correction insisted on is the shape that
> shipped. What the header still lacks is the versions, the scene ref and its
> hash; on-rails elements and per-system extension blocks have no place in the
> format yet either. That is the remaining gap between this block and the tree.
