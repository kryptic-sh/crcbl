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
- **Synthetic population driver**: headless, deterministic, reporting
  convergence and queue-time distributions. **Built, and it is a subcommand of
  the sample rather than of the engine CLI** —
  `bracket sim [--seed N] [--players N] [--ticks N]`, routed in
  `apps/bracket/src/main.rs` before the ordinary argument parser is reached, so
  a word that parser does not recognise is genuinely unknown rather than a
  command it forgot about. An earlier draft of this line wrote it as
  `crcbl bracket sim … --matches K`; there is no `bracket` subcommand on the
  engine CLI and the run length is in **ticks**, not matches, because a tick is
  what the matchmaker advances and the match count is an output of the run
  rather than an input to it.
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

## Where this stands

**Milestone 1's model, milestone 4's driver and milestone 5's web demo are
built.** `apps/bracket` has the queue with its widening tolerance, the Elo
ratings and the match stub, a UI-only client built on the draw list's
primitives, and `bracket sim` for the headless population soak;
`web/demos/bracket/` is the page and `bracket` is a row in `web/build.sh`'s
`DEMOS` array. Like `apps/sparks`, the demo takes **no input at all** — what a
visitor sees is a ladder sorting itself out and nothing they did — and every
decision, who queues this tick and who wins, comes from a hash over the seed and
a counter, so a run is reproducible from its seed on any platform.

**Two design points worth not re-deriving.** Pairing adjacent players on a
rating-sorted queue is the cheap form of the assignment problem and not an
approximation of it: the total gap over pairs drawn from a line is minimised by
pairing adjacent points, so no amount of searching finds a materially better
set, and it is `O(n log n)` for the sort — which is what lets thousands run in
CI. And `Rating` has **no constructor taking arbitrary points**: one starts
provisional and moves only through a step bounded by the K-factor, so a
non-finite rating has nowhere to enter from. That is the contract enforced
rather than documented; an `f64` parameter would have let a NaN in and it would
have spread through every later match.

**The convergence plot is on the page and it is the sample's actual argument** —
a rating system nobody can falsify is a number generator, so the distance
between what the ladder believes and what the players are really worth is drawn,
falling, on screen.

**Milestone 2 is blocked, and the blocker is narrower than "there is no UDP".**
There is no UDP transport and no LAN discovery — `crcbl-net` ships
`InMemoryTransport` and nothing else — but that is not what stands between this
sample and the thing it exists to prove. The demo runs its `Sim` directly rather
than over a transport at all, and routing it through a loopback **as things
stand would be worse than not doing it**: it would put the matchmaker behind a
tick-shaped input channel and look like the claim while not being it. Queueing,
leaving the queue and reporting a result are **commands** — each a request that
must be answered once and stay answered — and `crcbl-server`'s receive loop has
an arm for `ClientToServer::Command` whose body is empty, with a comment saying
so in as many words. So the missing piece is a way for a `GameModule` to receive
a command and reply to it, and it is engine work rather than sample work.

The multi-client half is absent for a second, independent reason: `Server` holds
one transport and one session manager, so "many connections, low bandwidth" has
no implementation to demonstrate. A browser demo could not show it either way,
since both ends live in one wasm module — it belongs with a native milestone.

**Rule 2 is therefore owed here rather than exempted.** This doc grants no
exemption from it and should not be read as taking one: `apps/bracket` opens no
`World` and implements no `GameModule` today, and the exit criteria below assume
it eventually will.

**Milestone 3 — identity, result signing and the rejection paths — is
unstarted.** `crcbl-net`'s `auth` module carries the per-session MAC every
post-handshake message takes, which is the ingredient; nothing signs a _result_.

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
