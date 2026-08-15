# Topic 23 — Netcode: Transports, Protocol, Foundations

The concrete network stack under the stage-4 transport seam: which transports
exist, the own UDP reliability layer, and the protocol foundations (handshake,
versioning, reconnect, hardening, bandwidth) that are cheap at P2 and misery to
retrofit.

## Supported stacks (LOCKED)

| Transport                 | Platform | Role                                                                                      | Lands           |
| ------------------------- | -------- | ----------------------------------------------------------------------------------------- | --------------- |
| **InMemory**              | all      | single player, tests, editor — the permanent local path; **the only plaintext transport** | P2              |
| **UDP + own reliability** | native   | the only network transport — **per-packet AEAD, always**                                  | P13 (design P2) |

**WebTransport and WebSocket are removed** (2026-08-09) — see the LAN correction
at the bottom of this file. Web builds are single player; the browser has no
network transport at all.

**Encryption rule (LOCKED)**: every packet on every network transport is
encrypted — no plaintext mode exists on the wire, no "disable crypto" flag.
InMemory is the sole plain path (it never touches a network).

**Status (2026-08-15): the rule binds P13 and nothing has been built against it
yet, because there is no network transport.** `Transport` is implemented by
`InMemoryTransport` and by `crcbl_store`'s `FileTransport` — a process-local
pair and a replay file, neither of which touches a socket. What _has_ shipped is
`crcbl_net::auth`: per-session HMAC-SHA256 keyed with the handshake's resume
token, truncated to 128 bits, plus a replay window. Its own header says it
"authenticates _and_ orders; it does not encrypt". That is not a breach of the
rule above — the rule is about network transports and there are none — but it
does mean **no confidentiality exists anywhere in the tree today**, and
`27-auth.md`'s opening premise, which describes itself as upgrading this
document's encryption, is written against a baseline that has not been built.
The key agreement and the AEAD arrive with the UDP layer, and that is where
taking on crypto dependencies becomes a real question.

Rejected: **TCP** (head-of-line blocking poisons the snapshot channel); **QUIC
from scratch** (TLS 1.3 from scratch = the wrong own-all-bugs; if a native QUIC
need ever materializes, `quinn` behind the seam is the sanctioned exception,
same policy as wasmtime).

All stacks implement the same channel semantics — consumers never know which
transport carries them:

| Channel              | Guarantees                         | Carries                                  |
| -------------------- | ---------------------------------- | ---------------------------------------- |
| unreliable-sequenced | latest-wins, drops fine, no resend | snapshots (old snapshot = worthless)     |
| reliable-ordered     | delivered, in order                | commands, events, chat, console          |
| reliable-fragmented  | delivered, reassembled             | bulk: join-in-progress snapshot, replays |

## Own UDP reliability layer (the classic, built properly)

Gaffer/netcode.io lineage — well-trodden gamedev territory, exactly the kind of
from-scratch this project is for:

- **One socket, channel-multiplexed packets**; per-packet header: protocol id,
  sequence, ack + 64-bit ack bitfield (piggybacked on every packet — reliable
  resend needs no separate ack traffic, and RTT measurement falls out free,
  feeding the topic 21 tick-lead estimate).
- **Resend**: unacked reliable payloads retransmit on RTO (RTT-derived);
  sequenced channel never resends by definition.
- **Fragmentation**: reliable channel only; conservative 1200-byte MTU start,
  optional discovery later.
- **Connection tokens** (netcode.io pattern): clients connect with a short-lived
  token minted by a trusted source (the server itself in direct-connect MVP; a
  backend later) — anti-spoof, cheap junk-traffic filter, and the key-material
  carrier when a backend exists.
- **Per-packet AEAD, from day one**: session keys established in the handshake
  via **X25519** key exchange (or delivered inside the connection token when a
  backend mints it); every packet after the hello is sealed with
  **XChaCha20-Poly1305** — nonce derived from direction + packet sequence
  (unique by construction, never reused), tag authenticates the header too
  (acks/sequence can't be forged). Rekey on reconnect.
  - **Crypto primitives are the sanctioned exception** (same policy as
    wasmtime/quinn): audited RustCrypto crates behind a seam — rolling your own
    cipher is the one from-scratch this project explicitly refuses. The
    _protocol_ (handshake, nonce discipline, framing) is ours; the primitives
    are not.
  - Trust model, stated honestly: direct-connect ECDH encrypts against passive
    snooping but is MITM-able without an authenticated root; token minting via a
    trusted backend (or operator-configured pre-shared key) upgrades to
    authenticated — that's the post-MVP auth story, and the seam for it exists
    in the token layer now.
- **Keepalive/timeout**: heartbeats on idle, drop detection windows, graceful
  disconnect message distinct from timeout.

## Protocol foundations (P2 — before any real socket exists)

Designed against InMemory first; every later transport inherits them:

- **Handshake**: hello → **key exchange** (X25519; skipped only by InMemory) →
  version gate → session accept. Exchanges protocol version, engine/game build
  ids, and the **component schema hash** (modules — topic 16 — make this
  mandatory: server and client must agree on every replicated schema; mismatch =
  clean refusal with a reason, never a decode crash). Everything after the hello
  rides the session keys.
- **Session identity + reconnect**: session token survives transport drops — a
  net blip resumes the session (entity bindings kept server-side for a grace
  window; catch-up = join-in-progress machinery). Distinct from a fresh join by
  design.
- **Hardening**: server treats every inbound byte as hostile — length-gated,
  schema-validated decode (fuzzed in CI, topic 12), per-client message-rate and
  byte-rate limits, oversized-message rejection. A malformed packet is a metric,
  never a panic.
- **Condition simulator**: a transport _wrapper_ (latency, jitter, loss,
  duplication, reorder) applying to any stack including InMemory — every sample
  is testable under bad network from P2 onward; the arena A/B tests are just
  presets of it.
- **Netgraph** (debug HUD, topic 7 at P10): RTT, jitter, loss, send/recv
  bandwidth, snapshot size, resend counts, tick-lead — per client on the server
  panel, self on the client.

## Bandwidth management (design now, tighten on demand)

- **Per-client budget**: bytes/tick cap; the snapshot writer already takes a
  client id — it gains a budget and a **priority model**: entities scored by
  (relevance × staleness), important ones update every tick, others rotate.
  Eventual consistency for the long tail, full consistency for what matters.
- **Quantization**: wire-format compression per component type — positions as
  sector-local fixed-point (a `WorldPos` structural win: sectors bound the
  range, so 16–24 bits/axis suffices), quaternions smallest-three, velocities
  half-float. Schema-declared, applied at snapshot encode; the determinism hash
  uses unquantized server state (quantization is a wire concern, not a sim
  concern).
- **Ack-baseline deltas (the design of record — supersedes "dirty-sets per
  tick")**: the server tracks each client's last-acked snapshot (free from the
  ack bitfield) and encodes every snapshot as a **delta vs that client's acked
  baseline**. This makes sparse sync automatic and loss-safe with zero game-code
  involvement:
  - unchanged since baseline → zero bytes;
  - lost packet, value changed again → old update never resent, current value
    ships in the next delta;
  - lost packet, value unchanged → still differs from baseline →
    auto-re-included ("desync detected" without detection logic — the baseline
    diff _is_ the detector). State never touches the reliable channel; discrete
    events (kills, wave-start) are the only reliable-ordered traffic. Server
    cost: bounded per-client snapshot ring + ack pointer — the same buffer
    prediction/rollback (arena) and the spectator/replay ring (topic 22) already
    want. Dirty-flags remain as a server-side _encoding accelerator_ (skip
    diffing clean systems), not the wire model.
- **Game code writes values; the engine syncs them.** Declaring a replicated
  component schema is the entirety of a game's netcode surface — no sync calls,
  no RPCs, no per-field flags in gameplay logic. (Modules — topic 16 — get this
  identically: engine owns their arrays.)
- **Delta encoding detail** (per-field vs whole-component) stays MVP-pragmatic:
  whole-component-on-change first, per-field masks when towers numbers justify;
  adaptive snapshot rate under sustained over-budget is the congestion response
  (drop to 30/20 Hz gracefully rather than queue).

## Galaxy-scale model: sectors are the wire architecture

The physics pillar made sectors the unit of space (topic 5: `WorldPos`, bubbles,
streaming). Networking adopts the same shape **as its architecture, not as a
later optimization** — a galaxy-capable sim with a whole-world wire protocol
would be a contradiction. MVP scenes simply occupy one sector, and every
mechanism below degenerates to "the whole scene" at zero cost.

- **Subscription is the primitive.** A client is subscribed to a set of sectors
  (its bubble + margin, server-controlled). _Everything_ it receives is scoped
  to that set: entity replication, events, scene chunks (stage 6 streaming and
  net subscription are the same concept — subscribe to a sector = get its scene
  data + its live entity stream + its physics activity). Sector = unit of space,
  physics, streaming, scenes, **and replication** — fifth consumer, one spatial
  system.
- **Snapshots are sector-partitioned**: per-(client, sector) ack-baseline
  streams. Subscribe = full sector state (join-in-progress machinery, scoped);
  unsubscribe = bulk destroy. "Full world snapshot" stops existing as a concept
  beyond one sector — which is why join, save, and replay all remain
  well-defined at any world size (they're sector sets).
- **Wire coordinates are sector-local always** (the quantization design above) —
  absolute galactic positions never cross the network; the subscription context
  provides the sector frame. Precision and bandwidth are both solved by the same
  structure.
- **Entity ownership + handoff as first-class**: every entity is owned by
  exactly one sector; crossing a boundary = explicit migration (trivial
  in-process — index move + `WorldPos` rebase, which physics does anyway).
  Formalizing migration now is the **server-meshing seam**: a sector ownership
  table maps sectors → sim instance; MVP = one sim owns all, later = multiple
  sim threads (topic 21 pool) or processes/machines own disjoint sector sets,
  with the same migration event crossing a transport instead of a pointer.
  Cross-server meshing itself stays post-MVP infra — but every protocol message
  being sector-scoped from P2 is what keeps it _possible_ instead of a rewrite.
- **Time at scale**: multiplayer bubbles share the one server tick clock (topics
  21/23 sync). On-rails regions (Kepler, topic 5) need no replication _at all_ —
  clients compute them analytically from orbital elements (tiny, near-static
  data); only live-bubble sectors stream. This is why a galaxy is cheap on the
  wire: almost all of it is equations, not state. Timewarp is a
  solo/single-bubble feature; a shared server never warps under connected
  clients with divergent bubbles.
- **Spectators/replays** (topic 22) inherit scoping: a spectator subscribes like
  a client (casters: to the action's sectors); recordings are per-sector streams
  — replay a battle without recording a galaxy.

Rollout: sector-scoped message envelope + (client, sector) baselines land at
**P2** (cost ≈ a key in a map while everything fits one sector); multi-sector
subscription activates with physics bubbles/streaming (P11, orbit is the proof);
relevance _within_ a subscribed sector (the priority/budget encoder above)
covers dense-sector scaling; cross-machine meshing = post-MVP infra behind the
ownership table.

## Topology stance (decided)

**Dedicated/listen server, direct connect, IPv4+IPv6, MVP.** Listen-server co-op
behind NAT needs traversal/relay — that is backend infra (with matchmaking,
accounts, server browser, relay fan-out) and stays explicitly out of the engine
core; the token mint is the interface those services will use. LAN discovery
(UDP broadcast beacon) is a cheap post-MVP nicety.

## Delivery

| Slice                                                                                                                                                                                                                  | Phase                                             |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| Channel semantics, handshake/version/schema-hash, session+reconnect, condition simulator, hardening + fuzz corpus, **sector-scoped envelope + (client,sector) ack-baselines** (galaxy wire model, degenerate 1-sector) | P2 (on InMemory)                                  |
| Netgraph HUD                                                                                                                                                                                                           | P10                                               |
| UDP reliability layer (acks, resend, fragmentation, tokens, **X25519+XChaCha20-Poly1305 AEAD**) + WebTransport + WebSocket(wss)                                                                                        | P13                                               |
| Quantization + priority/budget encoder                                                                                                                                                                                 | P13, tightened by towers co-op numbers            |
| Authenticated key roots (backend token mint / PSK), interest management, LAN discovery                                                                                                                                 | post-MVP                                          |
| Backend infra (NAT/relay/matchmaking/accounts/browser)                                                                                                                                                                 | out of engine core — separate project when needed |

## Testing (topic 12)

- Reliability layer soak under the condition simulator (loss up to 30%, reorder,
  dup): all reliable messages arrive ordered; sequenced channel never delivers
  stale-over-fresh.
- Fuzzed decode corpus in CI (malformed/truncated/hostile packets).
- Handshake matrix: version/schema mismatches refuse cleanly.
- Reconnect: scripted drop mid-session resumes within grace window with state
  hash continuity.
- Bandwidth: towers 4-player session stays under budget; numbers recorded.

## Risks

- **Reliability-layer subtleties** (ack wraparound, RTO tuning, fragment loss):
  the literature is rich and the condition-simulator soak is the gate; this is
  the best-documented from-scratch territory in gamedev.
- **Crypto misuse beats crypto strength**: primitives are audited crates; the
  risks that remain are ours — nonce discipline (sequence-derived, unique by
  construction, property-tested), rekey on reconnect, header authentication
  coverage. MITM caveat stands until an authenticated root (backend mint / PSK)
  exists — no ranked/competitive integrity claims before that.
- **Scope pull from backend infra**: the token mint is the boundary — the engine
  never grows matchmaking.

## Corrections (design review, 2026-07-27)

The ack-baseline model is the **Quake 3 delta-compressed snapshot** design —
named now, along with the two answers it supplies that were missing:

- **Entity lifecycle**: a delta is computed against the baseline's _entity set_,
  so **removals are expressed as part of the delta** (entities present in the
  baseline and absent now are encoded as destroys). "Unchanged → zero bytes"
  applies to surviving entities only. Asteroids' churn exercises this in P2's
  first week.
- **Baseline too old**: if a client's last ack has fallen off the bounded
  snapshot ring, the server sends a **full (keyframe) snapshot** and resets the
  baseline — the standard `delta-base-too-old` path, and the same encoder
  replays/join-in-progress already use.
- **Priority model**: the relevance×staleness rotation is the **Tribes 2
  priority-accumulator** design (Frohnmayer & Gift) — named so its
  starvation-avoidance rules are adopted rather than reinvented. Predicted
  components (26) are **exempt from rotation**: skipping their tick stalls
  reconciliation.
- **One-datagram rule**: since only the reliable channel fragments, a
  steady-state snapshot **must fit a single ~1200-byte datagram**. That is a
  hard contract on the budget encoder (≈576 kbps at 60 Hz), not a guideline;
  exceeding it means shedding by priority, never silently fragmenting.
- **Third traffic class added — unreliable events**: transient, high-rate,
  worthless-late messages (footstep/gunfire cues, impact VFX) must **not** ride
  reliable-ordered, where one lost packet head-of-line-blocks them into a stale
  burst. Channel table gains `unreliable-event`: tick-stamped, fire-and-forget,
  late = dropped.
- **WebSocket fallback**: TCP will head-of-line queue stale snapshots under
  loss; the sender applies **latest-only coalescing** (drop queued unsent
  snapshots on backpressure) rather than letting the queue snowball latency.
- **Encoded-space change detection**: "changed vs baseline" means _the encoded
  representation differs_, from P2 onward (identity codec until quantization
  lands at P13). Otherwise the P2 encoder is rewritten at P13 — and prediction
  comparison breaks (see 26).

## Correction (LAN-only networking, 2026-08-09)

**Sessions are LAN. There is no hosted infrastructure anywhere in the project,
and web builds have no networking at all.**

### What is removed

**WebTransport and WebSocket leave the plan.** Their sole purpose was browser
clients reaching hosted servers, and no hosted servers exist. The transport
surface is `InMemory` and UDP.

Browser multiplayer was examined rather than assumed away, and the obstacles are
structural rather than incidental:

- A browser **cannot listen on a socket** — there is no server-socket API of any
  kind, so a browser can never host.
- A browser **cannot discover hosts on a local network** — no UDP broadcast, no
  mDNS, no API. The lobby browser below is not implementable there.
- An **HTTPS page cannot open an insecure connection to a LAN address**, and a
  LAN host cannot readily hold a valid certificate for its address.

**WebRTC was considered and deferred, not refused.** Data channels with manually
exchanged connection codes need no signalling server and would map onto the
channel semantics above better than WebSocket did — the unreliable channel
survives. Against it: a third transport to maintain, a JS shim owning
`RTCPeerConnection`, a two-peer copy-paste flow no shipping game would use, and
STUN/TURN still needed off-LAN. Recorded in `docs/backlog.md` with its costs so
the decision can be reopened without re-deriving it.

Also removed with them: hosted matchmaking, relays, and topic 27's tier 3 — see
that document's own correction.

### What is added: LAN host discovery

The lobby browser the samples ask for, and the thing "lobby-lite" named without
specifying:

- **Hosts announce** on the local network at a modest interval — a small
  datagram carrying the protocol and schema hash the handshake already gates on
  (P2), the game and mode, the current and maximum player count, and whether a
  password is set. Broadcast and link-local multicast both, because networks
  disagree about which they forward.
- **Clients enumerate** by listening for announcements over a short window and
  presenting what replied, with entries ageing out when a host stops announcing.
  No central registry, no state on the client between runs.
- **Announcements are not trusted.** They advertise a host; they authenticate
  nothing. The existing handshake, schema-hash gate and per-packet AEAD do all
  the work they already do, unchanged. An announcement is a hint about where to
  connect and never a reason to skip a check.
- **Direct connect by address stays first-class** and is the path that always
  works: discovery is a convenience over it, not a replacement, and every
  sample's lobby offers both.

Testing: two processes on one host must find each other through the real code
path, and the discovery window must be shown to time out cleanly when nothing
answers — a lobby that hangs when the network is silent is the obvious failure
and the one nobody writes a test for.

### What this costs, stated plainly

Prediction and lag compensation (topic 26) are validated against **injected**
latency only — arena's fairness harness — because a LAN has almost none and
nothing in the project ever crosses the internet. That is reproducible and
CI-able, which is genuinely better for regression testing, and it means no
number in this project describes real internet conditions. Any claim that it
does would be unfounded.
