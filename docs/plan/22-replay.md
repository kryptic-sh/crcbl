# Topic 22 — State Recording: Replays, Debugging, Spectating

One recording system, three consumers: **debugging** (time-scrub, black-box
crash captures, determinism verification), **gameplay replays** (save, share,
re-watch), and **live spectating** (esports casting, delayed viewing). Built
almost entirely from machinery that already exists — the recording _is_ the
replication stream.

## Core insight: record the wire

The server already emits, every tick: snapshot deltas + events, tick-id stamped
(stage 4 + topic 21 tick sync). A recording is that stream written to disk:

```
.crpl container (versioned like topic-14 saves; same snapshot encoding)
  header: engine/format versions, scene ref+hash, tick rate, start tick
  index:  seekable table of keyframe offsets
  body:   [keyframe (full snapshot) every N ticks] + [per-tick deltas + events]
  tracks: optional side-tracks — input track (InputTickStates), marker track
          (kills/goals/custom game markers), caster/POV metadata
```

**What `crcbl-store` actually writes is the first version of that, and it is
flatter.** `crates/crcbl-store/src/replay.rs` owns the format:

```
magic  b"CRBLREPL", format_version u16, tick_count u64, tick_rate u32, start_tick u64
entries  TickEntry[tick_count]
TickEntry  tick_id u64, msg_len u32, msg_data — one encoded ServerToClient message
```

So: no keyframe index, no seek table, no side-tracks, and **no deltas at all** —
each entry carries the full server message for its tick. The header comment says
delta compression against the previous tick is what a future format bump may
add, which is the same versioning seam the sketch above assumes; the rest of
this section's container — the index, the marker and POV tracks — is still the
plan and nothing reads or writes it. Seeking, POV tracks and everything built on
them therefore describe work not yet started.

- **Keyframes** = full snapshots (the topic-14 save container, reused) every N
  ticks (~5 s): seeking = nearest keyframe + roll deltas forward.
- **Playback is just a client**: a replay viewer connects the normal client
  stack to a `FileTransport` instead of a socket — interpolation, rendering,
  audio, UI all behave identically. Zero special-cased presentation code.
- Server-side recording = authoritative truth, client-count agnostic, works
  headless (dedicated server records matches with no renderer in sight).
- Rewind = keyframe + fast-forward (deltas apply without rendering);
  fast-forward beyond real-time = same. Pause/step = trivial (playback owns the
  clock).

## The three consumers

### 1. Debugging (the sleeper feature)

- **Black box**: dev builds keep a rolling in-memory ring of the last ~30 s of
  stream; on panic/assert/`crcbl` signal it dumps a `.crpl` — every crash
  arrives with a replay of how it happened. Attach to bug reports; CI soak
  failures auto-attach theirs. **The automatic half is not built** — nothing
  installs `crcbl_store::crash_ring`'s `CrashRing` on a panic hook, and no CI
  job attaches one.
- **Time-scrub debugger** (generalizes the physics scrub): timeline UI in the
  debug tools — drag to any tick, inspector shows any entity's state _at that
  tick_, diff two ticks side-by-side (deterministic encoding makes diffs
  meaningful). The editor's play-mode gets it free (play sessions are recorded
  by default in dev).
- **Determinism verifier**: `crcbl replay verify` re-simulates from the input
  track + initial keyframe and hash-compares every tick against the recorded
  stream — **nondeterminism bugs locate themselves to the exact tick and
  system**. This turns the determinism pillar from a promise into a tool.
- `crcbl replay dump|diff|clip` — RON dump at tick, stream diffs, extract a
  tick-range into a standalone clip.

**What the CLI has today is one verb and no subverbs.** `ReplayArgs` in
`crates/crcbl-cli/src/args.rs` carries a file path and `--json`, and
`crcbl replay <FILE>` reads the container and reports its metadata. `verify`,
`dump`, `diff` and `clip` do not exist as words the parser knows — it rejects
any option it is not given — and each needs machinery this format does not carry
yet: `verify` needs the input side-track, `clip` needs the keyframe index. Those
two bullets are the plan, not a description.

### 2. Gameplay replays

- Record toggle (server command, so console/CLI/UI/game code all trigger it
  identically); auto-record matches = one config flag.
- Replay browser UI (engine-provided screen, CSS-styled like settings): list,
  watch, seek bar with marker track (game-defined markers — kills, wave-cleared
  — jump-to-moment).
- POV: the viewer is a free client — free camera by default; game supplies named
  POVs (follow player X) via the marker/POV track. Speed controls (0.25×–8×),
  frame-step.
- Sharing: one file, self-contained (scene ref + hash validates assets; asset
  _content_ is not embedded — replays assume the game build, version gates like
  saves).

### 3. Spectating (live replays)

- A spectator is a client whose stream is the **relay of the recording stream,
  delayed** — same `FileTransport` abstraction reading from a ring instead of a
  file. Server relays to spectator connections (or a relay node forwards,
  keeping player-server bandwidth clean — post-MVP infra).
- **Broadcast delay** (anti-ghosting): configurable N-second buffer, free — it's
  just read-cursor lag.
- Casters get the debugging toolkit pointed at entertainment: live timeline
  (pause/rewind the live match locally, then jump back to live), POV switching,
  free camera, marker jumps. The esports observer mode is the time-scrub
  debugger wearing a suit.
- Spectator count scales off-server via relays (the stream is one-way, fan-out
  friendly); interest management irrelevant (spectators get the full stream by
  design).

## Costs + limits (honest)

- Stream volume: deltas are already bandwidth-optimized for netcode; disk is
  cheaper than wire. Raw MVP, compression seam in the container (own LZ-class
  later if files annoy).
- Replays are **state recordings, not demos-by-input**: version-tolerant (any
  build with matching schemas plays them), seekable, spectate-able. The input
  side-track adds the determinism-verify superpower in dev, but playback never
  depends on re-simulation.
- Client-side POV recording (capture _my_ view incl. prediction misses) is
  post-MVP, after prediction exists.

## Delivery

| Slice                                                                | Phase                                                             |
| -------------------------------------------------------------------- | ----------------------------------------------------------------- |
| `.crpl` writer/reader, keyframes+index, `FileTransport` playback     | Keyframes and the index are still owed over `crcbl_store::replay` |
| Black-box ring + crash dump; record-by-default in dev/editor         | Nothing installs `crcbl_store::crash_ring`'s ring yet             |
| `crcbl replay` CLI (record/play headless/dump/diff/clip/verify)      | Every subverb beyond the metadata report is still owed            |
| Time-scrub debugger UI + marker track                                | P10 (with debug tools)                                            |
| Replay browser screen; determinism verifier in CI (soak runs verify) | P10                                                               |
| Live spectator relay + broadcast delay (rides dedicated server)      | P13 (towers marquee demo gains a spectator)                       |
| Esports observer polish (POV tracks, caster timeline), relay fan-out | post-MVP (arena era)                                              |

## Testing (topic 12)

- Roundtrip: record N ticks → play → per-tick state hash equals live run.
- Seek correctness: random seeks == linear playback state at same tick.
- Verify-tool self-test: injected nondeterminism (seeded) is caught at the right
  tick.
- Black-box: crash-during-write leaves a playable file (atomic segment writes,
  torn tail tolerated by reader).

## Risks

- **Schema evolution vs old replays**: same policy as saves (topic 14) —
  per-system versions, serde defaults, migration seam; replays one major version
  back are best-effort, older = politely refused.
- **Marker/POV track creep**: it's metadata, not logic — games write markers via
  one event API; the engine never interprets them beyond jump-to.
- **Relay infra scope**: MVP spectating = same-server connections; relay nodes
  are post-MVP infra listed in the multiplayer-infra gap, not smuggled in here.

## Correction (design review, 2026-07-27)

**"The recording IS the replication stream" was too literal.** The wire is
per-(client, sector) _ack-baseline_ deltas; a file has no acks and no client. A
recording is therefore a **tick-linear delta chain** (each tick delta'd against
the previous tick) plus keyframes — the same _codec_ as replication, different
baseline logic. `FileTransport` playback consequently requires the client's
delta-apply path to accept previous-tick baselines as well as acked ones. Small,
but it must exist at P2 rather than being discovered when the first replay is
written.

**Migration placement**: since `FileTransport` bypasses the handshake (and
therefore the schema-hash gate), version migration for older replays runs as a
**whole-file transcode** on load (not per-message decode) — one code path, and
it fails loudly on unsupported versions instead of half-playing.
