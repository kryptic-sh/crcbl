# Topic 23 — Netcode: Transports, Protocol, Foundations

The concrete network stack under the stage-4 transport seam: which transports
exist, the own UDP reliability layer, and the protocol foundations (handshake,
versioning, reconnect, hardening, bandwidth) that are cheap at P2 and misery to
retrofit.

## Supported stacks (LOCKED)

| Transport                 | Platform                   | Role                                                          | Lands           |
| ------------------------- | -------------------------- | ------------------------------------------------------------- | --------------- |
| **InMemory**              | all                        | single player, tests, editor — the permanent local path       | P2              |
| **UDP + own reliability** | native                     | primary game transport                                        | P13 (design P2) |
| **WebTransport**          | browser (+native optional) | QUIC datagrams=unreliable, streams=reliable — 1:1 channel map | P13             |
| **WebSocket**             | browser fallback           | reliable-only; unreliable channel degrades to reliable        | P13             |

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
  backend later) — anti-spoof, cheap junk-traffic filter, and the future
  auth/crypto seam in one mechanism.
- **Encryption, stated honestly**: browser transports have TLS inherently.
  Native UDP is plaintext in MVP; the token layer reserves the slot for
  per-packet AEAD (XChaCha-class) post-MVP. Competitive-integrity features
  should not pretend to exist before then.
- **Keepalive/timeout**: heartbeats on idle, drop detection windows, graceful
  disconnect message distinct from timeout.

## Protocol foundations (P2 — before any real socket exists)

Designed against InMemory first; every later transport inherits them:

- **Handshake**: hello → version gate → session accept. Exchanges protocol
  version, engine/game build ids, and the **component schema hash** (modules —
  topic 16 — make this mandatory: server and client must agree on every
  replicated schema; mismatch = clean refusal with a reason, never a decode
  crash).
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
- **Delta compression** stays as planned (dirty-sets MVP, per-field deltas
  post-MVP); adaptive snapshot rate under sustained over-budget is the
  congestion response (drop to 30/20 Hz gracefully rather than queue).

## Interest management (the intended design, for the record)

Sector-based pub/sub: a client subscribes to its bubble's sectors (the same
sectors physics/streaming/scenes already use — one spatial system, fourth
consumer). Entities entering/leaving subscribed sectors generate create/destroy
on the wire. Spectators (topic 22) subscribe to everything. Post-MVP as
scheduled; the SnapshotWriter client-id hook plus sector keys mean landing it
later touches encoding, not architecture.

## Topology stance (decided)

**Dedicated/listen server, direct connect, IPv4+IPv6, MVP.** Listen-server co-op
behind NAT needs traversal/relay — that is backend infra (with matchmaking,
accounts, server browser, relay fan-out) and stays explicitly out of the engine
core; the token mint is the interface those services will use. LAN discovery
(UDP broadcast beacon) is a cheap post-MVP nicety.

## Delivery

| Slice                                                                                                             | Phase                                             |
| ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| Channel semantics, handshake/version/schema-hash, session+reconnect, condition simulator, hardening + fuzz corpus | P2 (on InMemory)                                  |
| Netgraph HUD                                                                                                      | P10                                               |
| UDP reliability layer (acks, resend, fragmentation, tokens) + WebTransport + WebSocket                            | P13                                               |
| Quantization + priority/budget encoder                                                                            | P13, tightened by towers co-op numbers            |
| Per-packet encryption, interest management, LAN discovery                                                         | post-MVP                                          |
| Backend infra (NAT/relay/matchmaking/accounts/browser)                                                            | out of engine core — separate project when needed |

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
- **Plaintext window**: stated in docs and release notes until AEAD lands; no
  ranked/competitive claims before it.
- **Scope pull from backend infra**: the token mint is the boundary — the engine
  never grows matchmaking.
