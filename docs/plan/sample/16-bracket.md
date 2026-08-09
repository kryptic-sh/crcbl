# Sample 16 — bracket (ranked/matchmaking tech demo)

Matchmaking, rating and ranked session flow, **with no game attached**. Players
queue, get paired, a match is resolved by a stub, ratings move, the ladder
updates. Like hud is to the UI system: the isolated fixture for the service
layer.

## Why this exists as its own sample

breach and shard are **LAN only** — direct connect or local-network host
discovery, no hosted service. That is the right call for them and it strands
topic 27's tier 3, the `crcbl-mint` chain and every matchmaking concern with no
consumer, which sample rule 13 says is a topic that should not be built.

This sample is that consumer, and isolating it is the point rather than a
compromise. Matchmaking quality is a property of a **population over time**, not
of one session: a matchmaker that pairs well for four players and badly for four
thousand is a matchmaker nobody has tested. Attaching it to a real game would
mean needing a real playerbase to learn anything. With the match resolved by a
stub, a synthetic population of any size can be run in CI, deterministically, in
seconds.

**Networking is native and local, like every other sample.** A hosted
matchmaking service was considered and declined: it would have been the only
piece of infrastructure in the whole project, and the browser client it would
have enabled was the sole justification for WebTransport and the WebSocket
fallback existing at all. Both are dropped from topic 23 as a result — see the
LAN correction there.

So this sample's server is a process on the same machine or the same LAN, found
the same way breach and shard find theirs. What it still proves that nothing
else does is **reliable-channel, request/response traffic** — no snapshots, no
interpolation, no tick — which the protocol has never been driven by.

## Proves

- **Matchmaking as a service**: queue entry and exit, party formation, pairing
  by rating with a tolerance that widens as wait time grows, and the trade-off
  between wait time and match quality made visible rather than asserted.
- **A rating system that converges.** Elo-family, deterministic, and checkable:
  feed a synthetic population with known true skill and assert ratings converge
  within a stated tolerance over a stated number of matches. A rating system
  nobody can falsify is a number generator.
- **Identity and signed results** (topic 27): who a player is, and a result the
  host signs so it cannot be forged by a client — the chain breach gave up when
  it went LAN, at the tier a local host can actually back. Signature
  verification failing must reject a result, and that must be shown to fail.
- **Service-shaped networking** (topic 23): a long-lived server that is not a
  game server. Many connections, low bandwidth, reliable channel only, no
  snapshots, no interpolation, no prediction. This is the first consumer of the
  transport that is pure request/response, which is why it is worth having — the
  protocol has only ever been driven by tick-shaped traffic.
- **Population soak in CI**: thousands of synthetic clients queueing, matching
  and reporting, deterministically from a seed, as a nightly job.

## Scope

- **Server**: headless matchmaking service — queue, pairing, rating store,
  result validation, ladder query. Persists through topic 14 so a restart does
  not lose the ladder.
- **Client**: a UI-only application — sign in, queue, see the estimated wait,
  get matched, press a button to "play", see the rating change and the ladder
  position. Built with `crcbl-ui`, so it doubles as a second non-trivial
  consumer of the widget set after hud.
- **The match stub**: outcome resolved by a seeded roll weighted by the true
  skill of the participants. It is deliberately not a game, and it is
  deliberately not fair — an outcome that always favours the higher rating would
  make convergence trivial and prove nothing.
- **Synthetic population driver**:
  `crcbl bracket sim --seed N --players M --matches K`, headless, deterministic,
  reporting convergence and queue-time distributions.
- Web demo on the Pages site, single player like every other web build: client
  and matchmaking server in one wasm module over `InMemoryTransport`, queueing
  against the synthetic population. The matchmaker and the rating curve are
  fully demonstrable that way; only the transport is absent.

## Non-goals (hard cap)

An actual game of any kind, cosmetics, progression beyond rating, chat or
social, tournaments and brackets in the elimination sense (the name is about
pairing, not about a tree), anti-cheat, region/latency-aware routing, or a
production identity service. If a feature would need a real playerbase to
evaluate, it does not belong here.

**Exempt from sample rule 11** (`.crpix` art through the sprite pass): the
subject is a service and its client is a UI. Rule 4's debug panel applies and
its network module is the interesting one — this is the sample where the
netgraph reports service traffic rather than tick traffic, which is a shape
nothing else produces.

## Milestones

1. Queue + pairing + a rating that moves, all local over `InMemoryTransport`.
2. Real transport: native client to native server over UDP, found by direct
   address or LAN discovery.
3. Signed results: identity, result signing, rejection paths shown to fail.
4. Synthetic population driver + convergence assertions in CI.
5. Ladder persistence, restart survival, and the single-player web demo.

## Exit criteria

- A synthetic population of stated size converges to true skill within a stated
  tolerance over a stated number of matches — the numbers recorded here, from a
  run, not estimated.
- Queue-time versus match-quality curve recorded at several population sizes,
  including the degenerate small-population case where the matchmaker has to
  choose between waiting and pairing badly.
- A forged or unsigned match result is rejected, demonstrated by a test that
  fails when the check is removed.
- A native client completes the full flow against a server found by LAN
  discovery, and the same flow runs entirely in-process in the web build.
- The ladder survives a server restart with no lost or duplicated results.
- The netgraph shows service traffic sensibly — a panel built for tick traffic
  that reads as broken here is a finding about the panel.
