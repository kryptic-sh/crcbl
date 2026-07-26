# Topic 31 — Visibility Culling (Anti-Wallhack)

Server-side visibility-based replication filtering: a client **never receives**
the position stream of an enemy it cannot possibly see — wallhacks render
nothing because the data isn't there. Like the player kit (30): **optional,
first-class** — every competitive FPS/3PS registers it; towers, co-op PvE, and
anything without hidden-information stakes never does. FPS-era; the design lands
on the sector pub/sub (23) as a finer filter.

## Position in the stack

```
sector subscription (23)  →  coarse: which sectors a client gets at all
visibility filter (this)  →  fine: which combat-tagged entities inside those
                             sectors get a transform stream right now
priority/budget encoder   →  how often the visible ones update
```

- **Wire-relevance only, sim untouched**: like LOD's graphics-only rule, culling
  decides per-client _encoding_, never simulation — no determinism interaction,
  no tick-hash impact, spectators/replays (22) keep the full stream (their
  anti-ghost protection is _delay_, already designed).
- **Opt-in per entity** via a `vis_culled` tag: enemy players/AI = tagged;
  teammates, projectiles-in-flight, world objects = untagged (always replicate).
  Team relationships come from game data (the filter asks "does client C get
  entity E filtered?" through a game-supplied policy fn — engine mechanism, game
  policy, as always).

## The visibility test (two stages, budget-bounded)

1. **PVS pre-filter (optional bake)**: map bake computes
   potentially-visible-sets between cells from static occluders (`crcbl bake`
   grows `--pvs`; needs closed static occluder geometry — same content rule as
   penetrable meshes, 28). Different PVS cells that can't see each other → done,
   zero rays. Maps without a PVS bake skip to stage 2 (raycast-only mode works
   everywhere, just spends more rays).
2. **Raycast confirmation**: server casts rays (phys L0 batch, jobs pool) from
   the client's **eye envelope** to the target's **extremity points** (expanded
   hitbox corners + velocity-extruded). Any clear ray → visible. Budgeted per
   tick (rays/client cap, prioritized by distance + recently-visible-first);
   results cached with TTL; stale-while-budgeted fallback errs toward _visible_
   (gameplay correctness beats info hygiene when the budget runs out — a knob,
   but that's the sane default).

## Latency tolerance: envelopes, not positions (the hard part)

A naive point-to-point test pops enemies in mid-peek. The test uses **motion
envelopes** on both ends:

- Client eye envelope = predicted eye positions over the next
  `lead + jitter + margin` ticks (the 21/26 machinery knows the client's tick
  horizon) — where the client _could_ be looking from by the time this snapshot
  arrives.
- Target envelope = velocity-extruded extremities — where the target _could_ be
  by then.
- **Optic-aware FOV** (the 29 requirement, inherited): the envelope uses the
  client's maximum potential view (equipped magnified optic extends
  range/narrows-but-lengthens the relevant frustum) — scoped players never see
  pop-in.
- **Reveal-early, hide-late hysteresis**: transition to visible is immediate on
  any clear ray; transition to hidden waits a grace period (~250–500 ms) —
  smooths re-peeks and duel jitter. Grace = the deliberate, measured info leak.
- The tradeoff is a **knob with a meter, not a vibe**: bigger margins = smoother
  peeks + more leak. The leak auditor (below) turns the choice into numbers per
  map/mode.

## Server-authoritative audio mode (closing the audio side-channel)

Positional audio events are themselves a wallhack vector — a cheat reading event
coordinates pinpoints enemies through walls more precisely than any ear could.
The companion mode (paired with this filter; same optionality) removes positions
from the audio wire entirely:

- **Server spatializes, client renders**: the cue grammar (13) is a small
  deterministic parameter set, so the server computes it per listener per event
  and sends only
  `{sound_id, seed, ITD, per-ear gains, pitch cents, occlusion params, rolloff}`
  — **no world position in any audio message**. The client DSP applies
  parameters directly; the pure-f32 core doesn't care where the numbers came
  from.
- **The information-theoretic honest part**: audible direction _is_ direction
  data — the cap, not the elimination, is the win. Parameters are **quantized to
  human just-noticeable differences** (coarse ITD steps, ~1 dB gain steps,
  coarse cents): a cheat decoding the wire learns a fuzzy bearing cone — exactly
  what a skilled listener's ears extract, and nothing more. Versus raw
  coordinates (exact position, arbitrary precision), the leak collapses to
  perceptual resolution.
- **Rotation staleness** (client turns between server compute and playback): the
  wire carries **quantized listener-relative polar** (azimuth/elevation/distance
  — information-equivalent to the ear params); the client applies a rotation
  correction from its _own_ orientation delta since the compute tick. Flicks
  stay directionally correct; the leak stays capped at the quantized cone.
- Moving/looping sources: per-tick param updates for audible voices (a few bytes
  each), client interpolates — latest-wins like everything else.
- **Occlusion raycasts run server-side** in this mode (server has both
  positions) and **share the budget/cache with the visibility rays above** — one
  ray pool answers both "can you see them" and "how muffled are they."
- Own sounds stay client-local (13, unchanged); spectators/replays keep full
  positional streams (delay-protected, 22).
- **The server-cost price, stated plainly**: this mode moves per-listener
  spatialization math + occlusion rays from N clients onto the one server —
  O(listeners × audible tagged sources) per tick. It's small math (the grammar
  is a handful of ops per pair) and the rays share the vis pool, but it is real
  server load that client-side processing distributes for free. That asymmetry
  is exactly why the mode exists as a switch, not a default.
- **One feature gate (LOCKED)**: `competitive_integrity` — a single server
  config flag enabling **both** the visibility filter and server-authoritative
  audio together. They close the same leak (entity positions through walls) via
  two channels; shipping one without the other is a false promise, so the engine
  doesn't offer them separately. Gate off (default): client-positional audio,
  full replication — zero added server cost, right for co-op/PvE/towers. Gate
  on: both filters, budgets shared, leak auditor covering both channels.

## The other side-channels (all under the same gate)

Transforms and audio are the obvious two. A position leak through _any_ other
channel makes the filter theatre, so the gate closes all of them:

### VFX events (the biggest remaining hole)

Muzzle flashes, tracers, impacts, footstep dust and blood all carry world
positions — "muzzle flash at X,Y,Z" behind a wall is an exact fix, at arbitrary
precision, on every shot fired. Gunfire is constant and it is precisely what a
cheat wants to locate.

- Gate on: **VFX events are visibility-filtered like transforms** — an effect
  whose source is culled for client C is not sent to C at all.
- **Legitimately-visible consequences still play, without source coordinates**:
  a tracer crossing your window, muzzle light on your wall, impact sparks on
  your side of cover are real information a player would see — those are emitted
  as **effects at the visible location** (the impact point, the light's contact
  geometry) or as camera-relative visual-only params, never as "effect at the
  shooter's position."
- Impacts on _your_ geometry are visible by definition and pass through
  unfiltered — that's the intended peek-detection channel, same role audio
  plays.

### Streaming + preload timing

What the server asks you to load reveals where people are (the classic
preload-pattern leak):

- **Sector subscription is driven by your own bubble only** (already the 23
  design — restated as a security invariant): never demand-driven by another
  entity's presence.
- **Entity-driven asset loads are eager and batched, never on-demand**: loadout
  meshes, skins, weapon assets for all possible participants load at match start
  (or on roster join), so an enemy's appearance triggers zero new fetches. A
  first-appearance hitch is _also_ a leak.
- Wasm/`FetchSource` builds: same rule, and their fetch logs are visible to the
  page — batch-or-bundle is doubly required there.

### Bandwidth + timing side-channel

Even with perfect content filtering, packet size and rate correlate with nearby
entity count — measurable with off-the-shelf tooling, and used in practice
against shipped shooters:

- Gate on: **snapshot padding to bucket boundaries** (pad to the next bucket
  rather than sending a size that counts entities) + **rate smoothing** (fixed
  send cadence per client regardless of content).
- Cost is bandwidth — stated in the gate's docs and measured by the leak auditor
  (bytes wasted vs correlation reduction). Bucket granularity is the knob.

### Creation/spawn events + roster data

- **Create messages are filtered with transforms**: a spawn/join message
  carrying an initial position for a culled entity leaks exactly what the
  transform filter withholds. Entity creation for tagged entities is deferred
  until first visibility (the ack-baseline machinery handles late creation
  natively — 23).
- **Roster discipline**: PlayerIds (27) are fine; live per-player state in a
  scoreboard/roster broadcast is not. The engine ships identity, games must not
  broadcast health/position/status of enemies as "UI data" — the leak checklist
  covers it.

### The leak checklist (game responsibility)

Engine filters _its_ streams; games must not re-leak through theirs: minimap
markers, kill feed with locations, scoreboard/live stats, ping systems, voice
positional metadata, spectator streams handed to participating clients,
telemetry endpoints. Ships in the kit docs because every competitive team
rediscovers this list the hard way.

## Scope of protection (say it plainly)

The `competitive_integrity` gate stops **information wallhacks**: cheats that
read data the client shouldn't have. It does **not** stop aimbots, triggerbots,
or ESP built from legitimately visible data (an enemy in your line of sight is
on your screen and in your memory — necessarily). Nor is it anti-cheat software.
Games that need client-integrity enforcement add that separately; nothing here
conflicts. This paragraph is required in the gate's user-facing docs — a false
sense of protection is worse than none.

## What still gets through (by design)

- **Audio information at perceptual resolution** — capped as above. **No
  coordinates ≠ no information**: the grammar cone is the sanctioned channel,
  the same way visible impacts are.
- Visible-consequence VFX (impacts, tracers crossing your view).
- Reveal-early/hide-late grace windows (measured, tuned by the auditor).

## Debug + measurement

- **Server vis view**: debug overlay rendering what client X currently receives
  (the wallhack's-eye view — the honest way to verify the filter); per-client
  culled-entity counts in the netgraph (23).
- **Leak auditor**: records per tick `(sent, was-actually-on-screen)` for tagged
  entities → leak ratio + wasted-send ratio per map/mode — the number that tunes
  margins and graces. Runs headless over bot matches (24) in CI.
- `crcbl vis check <map>` — PVS bake stats, occluder-geometry lint (open meshes
  that break the bake get named, same loudness rule as 28).

## Testing (topic 12)

- Golden scenarios: wall-between → withheld; corner-peek at RTT presets →
  revealed ≥ N ms before first visible pixel (the pop-in bound, asserted per
  margin config); smoke/door-state cases if dynamic occluders enabled.
- Leak property: no tagged entity's transform is ever sent while outside the
  expanded envelope beyond grace (the anti-wallhack claim, as a test).
- Audio-mode leak property: in authoritative-audio mode, no message on the audio
  path contains world coordinates (schema-level assert), and param quantization
  steps meet the configured JND floors.
- **All-channel leak property** (the gate's headline test): over a scripted
  match, for every tagged entity and every client, **no message of any kind** —
  transform, create, VFX, audio, event — carries that entity's position while it
  is culled beyond grace. Enumerated by schema tagging (position-bearing fields
  are marked), so a new message type that leaks fails CI by construction rather
  than by reviewer memory.
- Streaming property: no asset fetch is triggered by another entity's first
  appearance (fetch log diffed across appearance events).
- Timing property: snapshot byte-size distribution is uncorrelated with
  visible-entity count beyond the bucket granularity (statistical test over
  bot-match traces); send cadence variance within bound.
- Perceptual equivalence golden: a scene rendered from server params vs from
  positions (reference mode) produces per-ear output within the quantization
  tolerance — the mode changes the wire, not the experience.
- Rotation-staleness bound: scripted flick at RTT presets keeps rendered
  direction error within the quantized cone (asserted).
- Budget property: ray spend per tick ≤ cap under bot-match load; starved budget
  degrades toward visible, never toward pop-in.
- Soak: bot matches (24) with the condition simulator (23) sweeping RTT — leak
  auditor numbers recorded per run.

## Delivery (FPS-era, after 26 lands — envelopes need the tick-horizon machinery)

0. **Schema position-tagging** (mark every position-bearing field across all
   message types) — the prerequisite that makes the all-channel leak property
   mechanically checkable rather than aspirational. Cheap, and done first.
1. Filter seam + `vis_culled` tag + policy fn + raycast stage w/ budget/ cache
   (raycast-only mode, works on any map); **transform + create/spawn filtering
   together** (creation deferred to first visibility).
2. Envelopes + optic-aware FOV + hysteresis/grace knobs.
3. **VFX event filtering** + visible-consequence emission rules (impact point /
   camera-relative params).
4. Server-authoritative audio mode (params, polar+rotation correction, shared
   occlusion rays).
5. Leak auditor (all channels) + server vis view + netgraph rows.
6. **Streaming discipline** (eager/batched entity assets, subscription
   invariant) + fetch-log property test.
7. **Padding + rate smoothing** with bucket knob; timing property test.
8. PVS bake + pre-filter + occluder lint.
9. Gate docs: scope-of-protection paragraph + leak checklist in the kit.

## Risks

- **Pop-in whack-a-mole**: the corner-peek golden with an asserted
  reveal-before-visible bound is the regression net; margins are data.
- **Ray budget vs player count**: PVS pre-filter exists precisely to keep ray
  counts sane on real maps; budget property test keeps it honest.
- **False security**: the filter stops position-stream wallhacks — it does not
  stop aimbots or game-side leaks. The docs say so plainly; the leak checklist
  scopes what "protected" means.

## Corrections (design review, 2026-07-27)

- **The JND claim was overstated.** Per _sample_, quantized ear-params leak no
  more than a perceptual cone — true. But a cheat **integrates** dozens of
  samples across a footstep sequence together with its own known motion and
  trilaterates a far tighter fix (a trivial Kalman filter; humans do not
  integrate this way). The honest claim is: _per-sample resolution is capped at
  human JND; sustained emitters remain estimable by aggregation._ Consequences:
  the **leak auditor measures an optimal estimator's position error over a bot
  match**, not per-message quantization, and repeated emitters get update-rate
  limiting plus dithered quantization.
- **The timing gate overrides adaptive snapshot rate.** 23's congestion response
  (drop to 30/20 Hz) is itself a content-correlated timing signal — precisely
  what the gate's fixed cadence removes. With `competitive_integrity` on:
  **fixed cadence + padded buckets always**, and congestion is handled by
  priority starvation only. The resulting bandwidth floor is stated as part of
  the gate's cost.
