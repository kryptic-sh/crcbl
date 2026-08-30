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
  is not optional. Keep last N autosaves (ring).

  **"Always" is a native always, and that is the trap in this bullet.**
  `crcbl_store::write_atomic` is `std::fs` all the way down: temp file opened
  `create_new`, `write_all`, `sync_all`, parent-directory fsync, rename, parent
  fsync again. The Origin Private File System has no `rename` to build that
  shape on, even through JS, so the browser path is `OpfsStorage` and it keeps a
  different half of the guarantee — torn reads prevented, **durability at return
  not provided**. A save that returned `Ok` in a browser is not yet a save that
  survives the tab closing. Do not carry a native atomicity test over to the
  wasm job and call it passed.

## Settings

- **Layered resolution**: engine defaults → game defaults → user settings file →
  CLI/env overrides. First-write wins upward; `settings.toml` stores only
  user-changed values (diff vs defaults — small files, upgrade-friendly). **All
  four layer kinds exist and only one is ever populated** (corrected 2026-08-27;
  this bullet said two existed). `SettingsLayer` in
  `crates/crcbl-store/src/settings.rs` has `EngineDefaults`, `GameDefaults`,
  `UserFile` and `CliOverrides` variants, `SettingsStack::add` appends any of
  them so a later layer wins, and a write always lands in the user file. What is
  missing is producers, not mechanism: `GameDefaults` and `CliOverrides` are
  constructed nowhere in the workspace but that module's own tests, and
  `SettingsStack::platform` returns a stack holding the player's file and
  nothing under it — which is what the `crcbl settings` CLI's one-layer stack is
  honest about.
- Namespaced: `[engine.video]`, `[engine.audio]`, `[engine.input]`, `[game.*]`
  free for the game. Typed access API with serde structs + defaults; unknown
  keys warn, never crash. **Every key in the first two namespaces is now
  enumerated** — see the settings catalogue below and the two topics it points
  at.
- Hot-apply where possible (volume, mouse sensitivity — immediate; vsync,
  resolution — apply-on-confirm pattern provided by the engine). **Neither half
  is built** (2026-08-27): there is no settings screen, so nothing has yet
  needed either, and "provided by the engine" describes a helper that does not
  exist. The live mechanisms both patterns would drive do exist —
  `GpuContext::set_pacing` reconfigures a running swapchain and
  `crcbl::engine::ModeRequest::toggle` switches display mode — so what is owed
  is the confirm-and-revert flow around them, not the applying.
- Key binds live in profile (RON) not settings TOML — they're per-player
  structured data (action → chord maps), and games extend the action set.

### The settings catalogue: keys are named before they are implemented (LOCKED 2026-08-27)

Two other topics enumerate the engine's player-facing settings in full —
[15-windowing.md](15-windowing.md) for display,
[39-capabilities.md](39-capabilities.md) for graphics quality, and
[13-audio.md](13-audio.md) for audio buses and output. This document owns the
part all three share: the file, the spelling, and the rule that lets them name a
key years before anything reads it.

**Rule 2 of the catalogue: a key with no implementation still gets its name and
its value domain now.** (Rule 1 is the clamp rule, and it lives in
[39-capabilities.md](39-capabilities.md).) A key is a compatibility surface the
moment one player's file contains it, and the two things that churn if it is
named late are the two things that must not — the settings screen's row identity
and the file on disk. Naming costs nothing: an unread key is exactly an absent
key, which `crcbl::settings::video_effects` already treats as "the player has
not asked" and which `SettingsStack::get` already answers `None` for. So the
catalogues are written whole and each row states its own implementation status,
rather than being grown a key at a time as passes land.

**The spelling convention is adopted, not invented.**
`crates/crcbl-store/src/settings.rs`' own module documentation already shows an
`[engine.video]` section carrying `vsync` and `resolution = [1920, 1080]` and an
`[engine.audio]` section carrying `master_volume`. Every catalogue key follows
it: bare snake_case nouns, no negated keys, one section per namespace. The
argument against negation is on `crcbl::settings::VIDEO_KEYS` — a settings file
describes what the player wants on, and a negated key would make
`shadows = false` and `no_shadows = false` both writable and opposite.

### What is actually missing, in three parts

The machinery under these catalogues is further along than "settings are not
built" suggests, and stating the gap loosely is how a plan comes to owe work the
tree already ships. None of the storage layer is owed. Verified 2026-08-30, the
gap is three specific things:

1. **The hot-apply-versus-apply-on-confirm pattern has one implementor.**
   `apps/options` writes through `crcbl::settings`' setters and
   `crates/crcbl-cli/src/settings_cmd.rs` writes from a terminal; nothing else
   in the workspace writes settings outside its own tests. So the bullet above
   describes a pattern one screen has had to implement and nothing has had to
   generalise.
2. **`crates/crcbl/src/settings.rs` reads sixteen keys and no more.**
   `VIDEO_KEYS` maps six booleans to `RenderEffects` bits and `GpuContext::open`
   reads them; `render_scale`, `frame_limit`, `antialiasing` and
   `anisotropic_filtering` are read beside them, and `audio_gains` reads the six
   `[engine.audio]` bus gains. That is the whole settings **surface** of the
   engine. Every other key in the three catalogues has a defined home in the
   TOML and no reader.
3. **The engine's own pause menu writes nothing.** It offers
   `MenuAction::Fullscreen` and `MenuAction::DebugOverlay` and neither touches a
   file — the fullscreen toggle is a live `Shell::set_mode` call whose result is
   forgotten at exit. `apps/options` is a sample; the engine-provided screen
   that turns this from a file a player edits by hand into a setting is the P10
   row in the delivery table below.

### When there is nowhere to write

**Browsers are the case worth deciding in advance, because it is not a failure
and must not be reported as one.** `crates/crcbl-store/src/lib.rs` records that
an IndexedDB fallback is still to come, so **OPFS is the only browser backend**.
`SettingsStack::platform` on `wasm32` asks `crate::web::opfs::installed()`, and
when no store is installed it logs at info level and returns an **empty user
layer** — every key reads as absent, which the clamp rule turns into "the player
has asked for nothing".

So the engine's behaviour is already right. What needs saying is what a **web
demo** should do on top of it, because "settings silently do not persist" is
indistinguishable from "settings are broken":

- **Keep running.** No start-up failure, no modal, no degraded render path — an
  absent key clamps nothing, so the frame is the one the game asked for.
- **Say so once, where the settings are.** A settings screen with no store
  behind it shows its rows and states that changes last for this session only. A
  screen that silently accepts a change it cannot keep is the failure mode this
  bullet exists to prevent.
- **Do not fall back to `localStorage`.** It is a second storage backend with
  different semantics reached from outside the `StorageSource` seam, and the
  fallback this crate is going to have is IndexedDB, behind that seam, where
  `OpfsEnvironment` can represent the choice. Adding a side door now is work
  thrown away and a second thing to keep in sync meanwhile.

### The catalogue's edges

A catalogue that quietly stops at the boundary of what its authors felt like
enumerating is worse than a short one, because a reader cannot tell the two
apart. So the settings a player expects to find that these catalogues do **not**
cover, and who owns each:

- **Input** — mouse sensitivity, invert Y, key binds, gamepad deadzone. Owned by
  [19-input.md](19-input.md), and this document already places the binds
  themselves in the profile rather than in `settings.toml`, because they are
  structured per-player data that games extend. Two cautions on that ownership,
  checked 2026-08-27: topic 19 names the deadzone (it is a per-device-kind
  response curve in its device layer, and appears in its binding sketch) but
  does **not** name a sensitivity or an invert-Y setting anywhere. Those have a
  home and no entry in it. `[engine.input]` is the namespace they would take.
- **Accessibility** — subtitles, text size, colourblind filters, reduced motion.
  **No topic owns these, and that is a gap rather than a deliberate omission.**
  Verified by searching every document under `docs/plan/`: not one mentions
  subtitles, colourblind filtering, text scaling or reduced motion. The only
  handling of any of it in the tree is `web/style.css`'s
  `prefers-reduced-motion` block, which belongs to the demo site's own pages and
  not to the engine. Whoever picks this up should expect it to be a topic of its
  own rather than a section bolted onto this one, because two of the four —
  subtitles and colourblind filtering — are engine subsystems (a caption
  presenter, and a post-chain filter that would join
  [39-capabilities.md](39-capabilities.md)'s catalogue) rather than keys.

### Considered and declined

- **A settings migration format beyond what this document already defines.** A
  settings file needs no versioned container and no `migrate` seam of its own:
  an unknown key is ignored, an unreadable value warns and clamps nothing, and
  an absent key means the player has not asked — which is already a complete
  answer for every skew a rename or a removal can produce. That is a property of
  a **clamp-only** layer and does not generalise to saves, which is why
  `SaveHeader` has a format version and `settings.toml` does not. The
  consequence to accept with it: a renamed key silently stops clamping, so
  renaming one is a compatibility break to avoid rather than a migration to
  write.
- **A per-monitor or per-adapter settings profile.**
  [15-windowing.md](15-windowing.md) carries the full reason. It belongs here
  too because the shape it would take is a persistence shape — a keyed set of
  files rather than one — and the answer is that if it is ever wanted, it is
  wanted as a `Profile` mechanism in this document, not as a second axis on the
  key space.

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
  (same wasm-forcing-function as assets). Browser saves are real saves. **No
  IndexedDB fallback exists** — OPFS is the only browser backend, so a browser
  without it has no persistence rather than a degraded one. That is a known
  hole, not an oversight, and `crcbl-store`'s `web` module says so in its own
  "what is not closed" list: the wasm side is a queue of `(name, bytes)` records
  that a shim could satisfy from IndexedDB without this crate knowing, but no
  shim does, and the choice has no representation in `OpfsEnvironment`.
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

| Slice                                                                                                                                                                                                                                                                                                                                                                                                                                           | Roadmap phase                                           |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `StorageSource` + settings layers (engine.video/audio/input)                                                                                                                                                                                                                                                                                                                                                                                    | P2 (with the sim core; settings needed by first sample) |
| Save/load container over snapshots + atomic writes + autosave — **not built over `SnapshotWriter` in practice**; see the note under the section heading.                                                                                                                                                                                                                                                                                        | P2 (thin layer over the same phase's SnapshotWriter)    |
| Profiles (high scores, binds) — **the high-score half built as `crcbl_store::record::Record`**; no `Profile` type, no RON, no persisted binds. See the profiles note above.                                                                                                                                                                                                                                                                     | P4 (breakout ships with a high score)                   |
| `crcbl save` CLI, save diff — **not built**: the verb is not parsed and `crcbl-cli` has no module for it. `crcbl-store`'s `save.rs` exists and nothing in the CLI reaches it. Blocked rather than merely owed: `save dump` is specified above to render a snapshot as RON, this tree has no RON reader or writer, and adopting one is an open decision in `docs/backlog.md`. Said P2 until 2026-08-23; the exit criteria below still assume it. | unbuilt                                                 |
| Settings UI screen (engine-provided, CSS-styled, reusable)                                                                                                                                                                                                                                                                                                                                                                                      | P10                                                     |
| towers: mid-run save/resume incl. co-op world save                                                                                                                                                                                                                                                                                                                                                                                              | S6                                                      |

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

> **The header still lacks the versions, the scene ref and its hash**, and
> on-rails elements and per-system extension blocks have no place in the format
> yet either. That is the remaining gap between this block and the tree.
