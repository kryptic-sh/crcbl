# Topic 27 — Auth: Identity, Tokens, Trust Roots

The authenticated root that upgrades topic 23's encryption from "private against
snooping" to "you know who you're talking to" — plus player identity, session
tokens, and the ranked-integrity chain. Engine owns the **mechanisms and the
token interface**; account systems stay backend territory (the consciously-out
line holds — but the interface is specified here, and a reference mint is
specified as a dev tool, though it is not built; see that section and the LAN
correction).

## Trust tiers (a server runs exactly one)

| Tier          | Trust root                           | Use                            | Guarantees                                                                                                      |
| ------------- | ------------------------------------ | ------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| **1 — Open**  | none (raw X25519 ECDH)               | LAN, dev, listen-server casual | encrypted vs passive snooping; MITM-able; identity = self-asserted                                              |
| **2 — PSK**   | operator-shared secret (out-of-band) | private/community servers      | mutual auth: handshake bound to the PSK (key confirmation) — MITM dead; identity = per-server accounts optional |
| **3 — Token** | backend keypair (Ed25519)            | official/ranked/public servers | full: authenticated identity, per-session keys, bans, ranked chain                                              |

The tier is server config; clients discover it in the hello. Engine code above
the transport never branches on tier — it sees "authenticated: yes/no, plus
PlayerId claims" and games gate features on that (ranked requires tier 3; the
topic 23 "no competitive claims" caveat resolves here).

> **None of the three tiers exists, and tier 1's row overstates even the
> baseline** (2026-08-27). `crcbl_net::handshake::Hello` carries a protocol
> version, an engine build id and a schema hash; there is no tier field, no
> `PlayerId`, and no notion of "authenticated" anywhere above the transport. And
> tier 1's guarantee column reads "encrypted vs passive snooping", which is
> stronger than the tree in two directions at once: nothing encrypts at all, and
> the one thing that _is_ built — `crcbl_net::auth`'s per-session HMAC — is
> keyed on a resume token that travels in the clear inside `Accept`, so a
> passive observer of the handshake can forge as well as read. The honest tier-0
> row today is "no confidentiality, and integrity only against an attacker who
> did not see the handshake".

## Tier 3: connect tokens (netcode.io model, hardened)

The flow that keeps game servers backend-independent at connect time:

1. Client authenticates to the **backend** over HTTPS (login method is backend's
   business: account, Steam, OAuth — outside the engine).
2. Backend mints a **connect token**, TTL ~30 s, single-use:
   - _private section_, encrypted+authenticated to the **server's registered
     key**: PlayerId, display claims, entitlement flags, session AEAD keys,
     expiry, client IP binding (optional knob);
   - _public section_ for the client: server address list, session keys
     (delivered over the HTTPS channel), expiry.
3. Client connects (topic 23 handshake) presenting the private section; the
   server validates **locally** — no backend round-trip on the connect path, no
   backend dependency during the match. Session AEAD keys come from the token
   (replacing raw ECDH in tier 3 — the keys are now _authenticated_ material).
4. Anti-replay: token nonce cache on the server (TTL-bounded); single-use
   enforced.

- **Key registry**: servers hold a keypair; the backend knows registered server
  public keys (registration = ops action). Backend root key signs nothing
  long-lived — compromise recovery = rotate + re-register, documented as a
  runbook from day one.
- **Reconnect** (topic 23 session resume) rides a session-scoped resume token
  derived from the session keys — no backend touch for a net blip.
- ~~Browsers: TLS already authenticates the server (WebTransport/wss certs).~~
  **Moot as of 2026-08-09**: browsers have no network transport, so there is no
  browser handshake to carry a token through. See this file's LAN correction.

## Player identity (engine-level)

- `PlayerId` = stable 64-bit id, minted by the backend (tier 3) or derived
  per-server (tier 2) or ephemeral (tier 1). Engine APIs (bans, stats hooks,
  replication client identity, replay/spectator attribution — topic 22 POV
  tracks) key on PlayerId; games never parse tokens.
- Display claims (name, cosmetic entitlements) arrive as signed token claims —
  the server trusts them without a lookup; richer profile data is a game/backend
  concern.
- **Bans/moderation**: server-local denylist by PlayerId (all tiers) + mint-time
  denial (tier 3, the real enforcement — banned identity gets no token).
  `crcbl admin` CLI surface (kick/ban/list) over the console command path —
  works on any server, headless included.

## Ranked-integrity chain (what tier 3 buys)

Authenticated identity → server-authoritative results **signed by the server
key** → reported to the backend → replays (topic 22) archived as evidence,
reviewable in the scrub debugger with rewind visualization (26). The engine's
anti-cheat stance stays honest: server authority + actions-only input + full
auditability; client-integrity software is explicitly out of engine scope (a
game may add third-party AC alongside — nothing here conflicts).

## Crypto inventory (all under the sanctioned-exception policy)

Ed25519 (signatures) + X25519 (ECDH) + XChaCha20-Poly1305 (AEAD) + BLAKE3-class
hash — audited RustCrypto crates behind the existing seam; the _protocols_
(token format, handshake binding, nonce/replay discipline) are ours and
property-tested. No TLS stack in the engine (browsers bring their own; native
uses the token/PSK constructions).

**None of that inventory has been taken on yet, and what shipped instead is
narrower.** `crates/crcbl-net/Cargo.toml` depends on `crcbl-core`,
`crcbl-shaders` and `thiserror` — no RustCrypto crate, no signature scheme, no
key agreement, no AEAD. `crates/crcbl-net/src/auth.rs` is what exists:
per-session **message authentication**, keying HMAC-SHA256 with the 32-byte
`ResumeToken` the handshake already exchanges, transmitting the MAC truncated
beside a replay counter, and rejecting a replayed counter through a sliding
`ReplayWindow`. The SHA-256 under it is the workspace's own, which is why the
only dependency added was `crcbl-shaders`.

**It authenticates and orders; it does not encrypt**, and its own header says so
in as many words. Every payload — snapshots, inputs, acks — stays readable on
the wire. So the opening sentence of this document is written against a topic 23
that does not exist yet: there is no "private against snooping" baseline for
tokens to upgrade, and confidentiality is a separate decision nobody has taken.
Adding it is where the AEAD and the key agreement above arrive, and that is the
point at which taking on the RustCrypto dependencies becomes a real question
rather than a plan.

## Reference mint (`crcbl-mint`, dev tool not product)

**Nothing ships. This section describes a crate that does not exist**, and the
LAN correction below is the reason: tier 3 has no deployment, so the mint has no
consumer and is not scheduled. The design is kept on paper, which is what the
correction says to do with it — so read the paragraph below as the specification
it would be built from, not as something in the tree.

_If built:_ a minimal token-mint service in-repo — file-backed identities,
mint + server-registry endpoints — enough to run tier 3 end-to-end in dev/CI and
for small communities, and it _is_ the executable spec of the backend interface
(a real backend reimplements its two endpoints). Explicitly not: accounts UI,
OAuth, scaling — that's the backend project.

## Testing (topic 12)

- Token properties: forged/expired/replayed/wrong-server tokens all refuse
  cleanly (fuzz + property suites); clock-skew tolerance windows.
- Active-MITM test: interposed handshake fails on tiers 2/3 (and _succeeds_ on
  tier 1 — documenting the tier honestly is a test too).
- Ban flow e2e: banned PlayerId refused at mint and at server denylist.
- Full tier-3 e2e in CI: `crcbl-mint` + dedicated server + headless client
  connect/reconnect/expire cycle.

## Delivery (post-MVP, wave 2 — with/before ranked-shaped games)

1. Tier plumbing in the handshake (hello advertises tier; engine API exposes
   `authenticated + PlayerId`) — lands with P13 transports so the seam never
   retrofits.
2. **PSK tier** (cheap, immediately useful for private servers).
3. Token format + server-side validation + nonce cache + `crcbl-mint`.
4. Bans/admin CLI; signed results + replay archival hooks.
5. Runbooks: key rotation, server registration, compromise recovery.

## Risks

- **Protocol-design errors beat primitive strength** (same as topic 23):
  token/handshake constructions follow the published netcode.io model closely
  rather than inventing; property tests + the MITM harness are the gate.
- **Backend-scope creep**: `crcbl-mint` is frozen at two endpoints + file
  storage; anything more is the backend project pulling engine resources.
- **Tier-1 false confidence**: the netgraph/server browser must label
  unauthenticated servers visibly — honesty in UI, not just docs.

## Corrections (design review, 2026-07-27)

- **Use the Noise Protocol Framework rather than a hand-rolled handshake.**
  X25519 + XChaCha20-Poly1305 with a bespoke transcript is exactly what Noise
  exists to make un-hand-rollable: `Noise_XX` (tier 1, mutual discovery),
  `Noise_XXpsk3` (tier 2, PSK-bound), `Noise_IK` (tier 3, known server key from
  the token) give verified key confirmation, rekey, and transcript binding over
  the same RustCrypto primitives (WireGuard lineage). Our protocol work stays
  the token format and framing; the handshake pattern is standard.
- **Tier 2 PSK needs a stated entropy rule**: a human-chosen passphrase bound
  into the handshake is offline-guessable from a passive transcript. Either
  require high-entropy keys (generated, not typed) **or** use a PAKE (**SPAKE2**
  / CPace). Decision: high-entropy generated PSKs only; PAKE is the documented
  upgrade if community servers want passphrases.
- **Nonce/sequence width is specified**: 64-bit sequence, transmitted truncated
  with **DTLS 1.3-style implicit reconstruction**, epoch bump on rekey. Wrap
  behavior is defined rather than left to discovery.

## Correction (LAN-only scope, 2026-08-09)

**No hosted infrastructure exists in this project, so tier 3 as written has no
deployment.** Sessions are LAN — direct connect by address, or a host found
through local-network discovery (topic 23's correction) — and web builds have no
networking at all. What this document specifies is not wrong; what changes is
which parts have a consumer.

### What survives, and where it is proven

- **Tier 0/1 (open and pre-shared key) are the game tiers.** A LAN host is the
  authority, a password is a PSK, and the handshake, schema-hash gate and
  per-packet AEAD from topic 23 do everything they already did.

  ~~breach and shard both run here.~~ **Both shipped, and neither runs here**
  (2026-08-27). `apps/breach` is single player — one hitscan pistol, a firing
  range and a bot arena, `crcbl` with the `greybox` feature and a **loopback**
  session over `InMemoryTransport` (its `game.rs` runs the real handshake and
  waits for `SessionState::Connected`, against a server in its own process) —
  and `apps/shard` says in its own app module that it has no network section for
  the same reason. So the tier that both actually exercise is the one this
  document does not list: **the in-process one, where there is no wire and the
  question does not arise.** The sentence is kept struck rather than deleted
  because it names the intent — when either sample grows a real session, tier
  0/1 is where it lands.

- **Player identity stays**, scoped to a host rather than to a service: who you
  are on the machine you joined. shard's characters belong to the shard they
  were made on, and there is no cross-server transfer to design. Unbuilt at
  engine level: no crate under `crates/` defines a `PlayerId`, and shard's
  character is a local save file with no identity attached to it. The one
  `PlayerId` in the tree is `apps/bracket`'s own `queue::PlayerId`, a `u32`
  index into its simulated population — a sample type, not the engine one this
  section means.
- **Signed results survive, at the tier a local host can back.** bracket
  ([sample/16-bracket.md](sample/16-bracket.md)) is the named consumer: the host
  signs a match result so a client cannot forge one, and a forged or unsigned
  result must be rejected by a test that fails when the check is removed. That
  is the useful half of the ranked chain and it needs no service.

  **`apps/bracket` shipped without it** (2026-08-27), and the reason is worth
  recording rather than filing as a miss: bracket is matchmaking, rating and
  ladder over a **population**, with each match resolved by a stub so a
  population of any size runs deterministically from a seed. It has no host, no
  client and no transport — nothing in it imports `crcbl-net` — so there is no
  result crossing a trust boundary to sign. Signed results still need a
  consumer, and bracket will only become one if it grows a real match on the
  other end of a real session.

### What is deferred for want of a consumer

**Hosted tier 3, the ranked-integrity chain and `crcbl-mint` as a running
service.** breach was their only consumer and breach is LAN, so per sample rule
13 — a topic that can name no adopting sample is not ready to be built — they
are not scheduled. The design is kept because it is the expensive half to get
right and cheap to keep on paper, and because the token layer in topic 23 is
already the seam it would arrive through.

**What would change it:** a decision to host anything. That decision was taken
deliberately in the other direction — see the samples overview — and reversing
it is a product call rather than a technical one.

### The honest note

Tier 3's value is a chain of custody a player can trust when they do not trust
the host. On a LAN the host is usually a friend in the room, which is why the
tier degrades gracefully into "the host is the authority" rather than into
nothing. It also means **this project never demonstrates a trust model where the
host is adversarial**, and no claim to the contrary should be made from what it
does demonstrate.
