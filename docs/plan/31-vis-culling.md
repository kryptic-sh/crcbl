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
- Default mode remains client-positional events (simpler, right for co-op/PvE);
  competitive games flip vis-culling + authoritative-audio together.

## What still gets through (by design — and catalogued)

- **Audio information at perceptual resolution** — by design, capped as above.
  The distinction: **no coordinates ≠ no information** — the grammar cone is the
  sanctioned channel.
- Create/destroy churn rides the 23 machinery (visibility flip = same wire
  semantics as sector enter/leave; ack-baselines handle it).
- **The leak checklist** (engine docs, game responsibility): minimap markers,
  kill feed, scoreboard pings, voice positional data, spectator streams handed
  to clients — the engine filters _its_ stream; games must not re-leak through
  theirs. The checklist ships in the kit docs because every competitive team
  rediscovers it the hard way otherwise.

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

1. Filter seam + `vis_culled` tag + policy fn + raycast stage w/ budget/ cache
   (raycast-only mode, works on any map).
2. Envelopes + optic-aware FOV + hysteresis/grace knobs.
3. Leak auditor + server vis view + netgraph rows.
4. PVS bake + pre-filter + occluder lint.
5. Audio-position quantization knob; checklist docs in the kit.

## Risks

- **Pop-in whack-a-mole**: the corner-peek golden with an asserted
  reveal-before-visible bound is the regression net; margins are data.
- **Ray budget vs player count**: PVS pre-filter exists precisely to keep ray
  counts sane on real maps; budget property test keeps it honest.
- **False security**: the filter stops position-stream wallhacks — it does not
  stop aimbots or game-side leaks. The docs say so plainly; the leak checklist
  scopes what "protected" means.
