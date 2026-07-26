# Sample 11 — breach (FPS-era flagship)

5v5 round-based competitive shooter: CS2-style structure (rounds, buy phase,
bomb plant/defuse) with Tarkov-flavored gunplay (real ballistics, penetration,
armor, limb/organ damage). Four modes on shared systems. This is the sample that
consumes the entire competitive stack — the FPS-era equivalent of what towers is
to the MVP era, and the reason topics 26–31 exist.

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
- **Auth tier 3** (27): ranked-shaped session flow via `crcbl-mint`;
  server-signed results.
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
- No progression, no cosmetic economy, no matchmaking (direct connect +
  `crcbl-mint` dev tier 3), no clan/social systems.

## Non-goals

Extraction-raid mode (Tarkov's actual loop), open-world maps, vehicles,
destructible geometry, weapon modding depth beyond optics, seasonal content,
monetization anything.

## Milestones

1. **Duel test**: 1v1 on a test map — kit movement, ADS, ballistics, prediction,
   lag comp; fairness harness green at 3 RTT tiers.
2. **Defuse core**: rounds, buy, plant/defuse, 5v5 with bots, HUD.
3. **Gate on**: `competitive_integrity` enabled; leak auditor numbers recorded
   per map; server load measured with authoritative audio.
4. **Modes**: KotH, DM, TDM as rule modules.
5. **Health/armor depth**: organs, bleeding, meds, armor degradation.
6. **Spectator + casting**: observer POV switching, kill review with rewind viz;
   match replays archived.
7. **Browser client**: wasm build joins native servers (Tier B budget recorded —
   the "competitive shooter in a browser" demo).

## Exit criteria

- A full 5v5 defuse match (mixed humans + bots, native + browser clients) on a
  dedicated tier-3 server, replay archived, spectator watching with broadcast
  delay.
- Fairness: hit% vs ping curve flat within tolerance across 20–150 ms; recorded
  in the doc.
- Leak audit: with the gate on, the all-channel leak property holds over full
  matches; measured leak (grace-window + perceptual audio cone) is documented
  per mode.
- Perf: server tick budget at 10 players + bots with gate on, recorded; client
  frame budget with PiP scope active, native + Tier B.
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
