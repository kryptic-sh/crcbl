# Sample 11 — breach (FPS-era flagship, web slice then native competitive)

5v5 round-based competitive shooter: CS2-style structure (rounds, buy phase,
bomb plant/defuse) with Tarkov-flavored gunplay (real ballistics, penetration,
armor, limb/organ damage). Four modes on shared systems. This is the sample that
consumes the entire competitive stack — the FPS-era equivalent of what towers is
to the MVP era, and the reason topics 26–31 exist.

## Two milestones, and the competitive build is native only

**Milestone 1 — the web slice.** A single-player cut — a firing range and a bot
practice map — that builds for `wasm32` and ships on the Pages site. Rasterised
lighting, `IndirectPerBatch` geometry, `ArrayPages` materials: every fallback
path, on first-person content, before any native-only feature is layered over
it. Same reasoning as `shard` ([15-shard.md](15-shard.md)) — the constrained
target is built first so the fallbacks are proven rather than assumed.

**Milestone 2 onward — the native competitive game**, and it does not ship a
browser build. Four things make a browser build of the competitive game not
merely worse but _wrong_, and they are not degradations the capability model can
absorb:

- **Anti-cheat is structurally impossible.** A wasm client is dumpable and its
  memory readable, with no attestation of any kind. Topic 31's server-side
  visibility filtering survives and is the good half — the server never sends
  what a client should not see — but topic 27's tier 3 and any client integrity
  claim mean nothing in a browser.
- **No raw mouse input.** Pointer Lock reports accelerated, browser-smoothed
  deltas. `RAW_POINTER_MOTION` is the capability this genre is built on, and the
  web shell cannot honestly report it.
- **The unreliable channel degrades to reliable** on the WebSocket fallback —
  head-of-line blocking on state updates is a gameplay difference, not a perf
  one.
- **Latency that cannot be measured or removed**: rAF pacing, no present
  feedback, the compositor in the path.

Everything the browser _can_ honestly do, milestone 1 does. What milestone 2
adds is a competitive game, and a competitive game in a browser would be a claim
the platform cannot back. `shard`'s milestone 2 carries networking on native
too, at scale rather than at latency.

## Multiplayer is LAN, and that is deliberate

**Sessions are direct-connect by IP, or found through a lobby browser that
discovers hosts broadcasting on the local network.** No hosted matchmaking, no
relay, no ranked ladder, no accounts service.

This is a scope decision, not a limitation to apologise for, and it buys a lot:
the netcode is proven against real packet behaviour without anyone running
infrastructure; a LAN's low and stable RTT is where prediction and lag
compensation can be _validated_ before they are stressed; and the fairness
harness (topic 26) can inject latency deliberately rather than depending on
whoever happens to connect.

What it removes from this sample: ranked-shaped auth (topic 27 tier 3), a
matchmaking service, and the server-signed result chain — all of which move to
`bracket` ([16-bracket.md](16-bracket.md)), where they can be tested against a
synthetic population instead of needing a real playerbase. The **integrity gate
(31) stays** — it is a property of what the server sends, not of who is allowed
to connect, and it is the more interesting half.

What it adds as engine work: **LAN host discovery** — hosts announce on the
local network, clients enumerate them — which topic 23's "lobby-lite" names but
does not specify. See that topic for the shape.

## Proves (the whole competitive spine, as one game)

- **Prediction + lag comp** (26): 5v5 duels at real RTTs; fairness harness
  numbers (hit% vs ping) are gates, not vibes.
- **Ballistics + kinetic impact** (28): penetration through map materials, armor
  with behind-armor blunt trauma, limb/organ hit chains, falling and
  vehicle-less kinetic cases (drops, thrown grenades).
- **First-person rendering** (29): viewmodel pass, ADS, PiP magnified optics,
  full-sync feet/cosmetics/audio across 1P/3P/spectator.
- **Player kit** (30): movement adopted from the kit (milsim preset), proving
  the kit is production-grade, not a toy.
- **`competitive_integrity` gate** (31): the full leak surface closed —
  transforms, VFX, audio, streaming, timing — with the leak auditor reporting
  per map/mode.
- **Session trust on a local network** (27): the host is the authority and the
  trust tier is PSK-shaped rather than ranked — see "Multiplayer is LAN" above.
- **Replays/spectating/casting** (22): every match recorded; observer mode with
  POV switching and the rewind visualizer for disputed kills.
- **Audio cue grammar under maximum pressure** (13): the esports legibility
  claim tested by people trying to win.
- **Navigation** (24): bots for warmup, backfill, and CI soak matches.

## Modes (shared systems, thin rule modules)

| Mode                 | Rules                                                       | Why it's here                                     |
| -------------------- | ----------------------------------------------------------- | ------------------------------------------------- |
| **Defuse** (5v5)     | Round-based, buy phase, plant/defuse, one life per round    | The flagship mode — economy, rounds, clutches     |
| **King of the Hill** | Contest a moving/fixed sector; longest cumulative hold wins | Sector-scoped objectives; tests area contention   |
| **Deathmatch**       | FFA, instant respawn, no economy                            | Warmup + the netcode stress mode (constant churn) |
| **TDM**              | Team score to limit, respawn waves                          | Team play without round overhead                  |

Each mode is a **rule module** (topic 16) over one shared combat/movement/
economy core — the sample doubles as the demonstration that modes are data + a
small module, not forks of the game.

## Gunplay (Tarkov-flavored, engine-honest)

- Real projectiles: mass/velocity/drop/drag; penetration chains through walls
  and bodies; ricochet; deposition-driven blunt damage behind armor.
- Armor: plates with class/durability, degradation from deposited energy — game
  data over the engine's hit chains (28's truncation contract).
- Health: limb + organ model (nested hitboxes), bleeding, tourniquets, meds —
  pure game-layer components, the sim-health proof.
- Weapon handling: ADS with per-optic sockets, recoil patterns (learnable,
  deterministic — the same philosophy as the audio grammar), weapon
  inspect/reload/malfunction states via the anim state machine.
- Inventory: grid inventory with drag-drop (the UI capability this sample drives
  into topic 7).

## Scope (hard caps — it's a sample, not a product)

- **2 maps** (editor-authored: one defuse layout, one small KotH/DM arena).
- **6 weapons** (2 SMG, 2 rifle, 1 sniper w/ PiP optic, 1 pistol), 3 armor
  classes, ~6 consumables.
- 5v5 max; bots fill empty slots.
- Economy: simplified CS-style buy (kill/round rewards, no skins/trading).
- No progression, no cosmetic economy, no clan/social systems. Sessions are
  direct-connect by IP or found on the LAN — no matchmaking service.
- **Debug panel on, network module included** (sample rule 4). Ten clients on a
  real transport with prediction, lag comp and an integrity gate is the widest
  the netgraph ever gets asked to be, and the buy menu, the inventory grid and
  the scoreboard are where the panel has to coexist with a dense game UI rather
  than float over an empty frame.
- **`.crpix` art for the 2D layer** (sample rule 11): the grid-inventory kit's
  item icons above all — topic 34's grid is item _shapes_, so every item is a
  hand-drawn sheet with known texel dimensions, which is exactly what the format
  is for — plus the buy menu, the killfeed and the scoreboard. The maps, weapons
  and players are 3D.

## Non-goals

Extraction-raid mode (Tarkov's actual loop), open-world maps, vehicles,
destructible geometry, weapon modding depth beyond optics, seasonal content,
monetization anything.

## Milestones

0. **Web slice**: firing range + bot practice map, single player, rasterised
   lighting and both geometry fallbacks, shipped on the Pages site. First-person
   rendering (29) and the weapon kit (38) on the constrained target, with a
   recorded browser budget.
1. **Duel test**: 1v1 on a test map — kit movement, ADS, ballistics, prediction,
   lag comp; fairness harness green at 3 RTT tiers.
2. **Defuse core**: rounds, buy, plant/defuse, 5v5 with bots, HUD.
3. **Gate on**: `competitive_integrity` enabled; leak auditor numbers recorded
   per map; server load measured with authoritative audio.
4. **Modes**: KotH, DM, TDM as rule modules.
5. **Health/armor depth**: organs, bleeding, meds, armor degradation.
6. **Spectator + casting**: observer POV switching, kill review with rewind viz;
   match replays archived.

## Exit criteria

**Milestone 0 (web slice)**

- Firing range and bot practice playable in a browser from the same build that
  runs natively, with the summary line naming the paths it selected.
- Golden frames per `GeometryPath`, and a recorded browser budget for
  first-person content.

**Milestone 2 onward (native competitive)**

- A full 5v5 defuse match (mixed humans + bots, all native clients) on a
  LAN-hosted dedicated server found through the lobby browser, replay archived,
  spectator watching with broadcast delay.
- Fairness: hit% vs ping curve flat within tolerance across 20–150 ms; recorded
  in the doc.
- Leak audit: with the gate on, the all-channel leak property holds over full
  matches; measured leak (grace-window + perceptual audio cone) is documented
  per mode.
- Perf: server tick budget at 10 players + bots with gate on, recorded; client
  frame budget with PiP scope active, native ray-traced and native rasterised.
- **Feel**: gunplay and movement good enough that people ask to play it again —
  the only exit criterion here that isn't a number, and the one that matters
  most for a flagship.

## Risks

- **Scope**: a competitive shooter is the genre most prone to infinite polish.
  The caps above are the contract; anything beyond is a separate game project,
  not sample work.
- **Content cost**: 2 maps + 6 weapons + rigs is real art time — CC0/kit assets
  first, custom only where the systems need proving.
- **It will find engine bugs at a rate no other sample does** — that's the
  point; budget engine-fix time inside this sample's schedule, not after.
