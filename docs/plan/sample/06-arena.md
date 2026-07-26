# Sample 06 — arena (post-MVP)

Multiplayer twin-stick arena shooter, 2–8 players. Deliberately the sample the
MVP netcode _cannot_ serve well — it exists to force the post-MVP netcode work
(client prediction, lag compensation) with a real consumer, not speculation.
**Not started before MVP ends.**

## Proves (each = a post-MVP engine feature it pulls in)

- **Client-side prediction + reconciliation**: twitch movement feels bad at
  interpolation-only latency; arena is the forcing function for the prediction
  hooks stage 4 left open. Acceptance = blind A/B (prediction on/off) is
  night-and-day at 100 ms simulated RTT.
- **Lag compensation** (server-side rewind for hit checks): hitscan weapons need
  it; the stage 4 tick-determinism/state-hash foundation becomes the rewind
  buffer.
- **Interest management / delta compression**: 8 players + projectile spam
  produces the bandwidth numbers horde only simulated.
- **Network condition tooling**: simulated latency/jitter/loss as a first-class
  engine debug feature (transport-seam shim), driven by needing it daily here.
- **Prediction-aware game code ergonomics**: what does a system author write to
  opt into prediction? Arena answers with real code; the answer becomes engine
  docs.

## Scope

- 1 symmetric arena map (editor-built), top-down-ish camera, WASD + aim.
- 2 weapons (hitscan, projectile), health/respawn, FFA deathmatch to score
  limit. 2–8 players, native + browser clients.
- Bots (dumb roam+shoot) so netcode testing doesn't require 8 humans — doubles
  as headless load-test client harness (engine tool fallout).

## Non-goals

Teams/modes beyond FFA, progression, cosmetics, matchmaking/server browser,
voice/chat beyond a minimal text line, ranked anything.

## Milestones

1. Interpolation-only version (works day 1 post-MVP, feels floaty at latency —
   the recorded "before").
2. Prediction + reconciliation shipped in engine; arena adopts; A/B recorded
   (the "after").
3. Lag comp for hitscan; fairness validated with scripted bot duels at
   asymmetric RTTs.
4. 8-client soak (bots, mixed native/browser) on dedicated server; bandwidth/CPU
   numbers recorded here.

## Exit criteria

- Playable-feeling at 100 ms RTT with prediction on (subjective bar made
  objective: input-to-visible-response < 1 render frame for own movement).
- Netcode debug HUD (RTT, reconciliation corrections/sec, snapshot bandwidth)
  shipped as engine feature, not sample code.
- The prediction/lag-comp engine APIs used here are documented from this
  sample's code, same pattern as towers-documents-ECS.
