# Topic 27 — Auth: Identity, Tokens, Trust Roots

The authenticated root that upgrades topic 23's encryption from "private against
snooping" to "you know who you're talking to" — plus player identity, session
tokens, and the ranked-integrity chain. Engine owns the **mechanisms and the
token interface**; account systems stay backend territory (the consciously-out
line holds — but the interface is specified here, and a reference mint ships as
a dev tool).

## Trust tiers (a server runs exactly one)

| Tier          | Trust root                           | Use                            | Guarantees                                                                                                      |
| ------------- | ------------------------------------ | ------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| **1 — Open**  | none (raw X25519 ECDH)               | LAN, dev, listen-server casual | encrypted vs passive snooping; MITM-able; identity = self-asserted                                              |
| **2 — PSK**   | operator-shared secret (out-of-band) | private/community servers      | mutual auth: handshake bound to the PSK (key confirmation) — MITM dead; identity = per-server accounts optional |
| **3 — Token** | backend keypair (Ed25519)            | official/ranked/public servers | full: authenticated identity, per-session keys, bans, ranked chain                                              |

The tier is server config; clients discover it in the hello. Engine code above
the transport never branches on tier — it sees "authenticated: yes/no

- PlayerId claims" and games gate features on that (ranked requires tier 3; the
  topic 23 "no competitive claims" caveat resolves here).

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
- Browsers: TLS already authenticates the _server_ (WebTransport/wss certs); the
  connect token adds _client_ identity — same token, fetched over HTTPS,
  presented in the WT handshake. One model, all transports.

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

## Reference mint (`crcbl-mint`, dev tool not product)

A minimal token-mint service ships in-repo: file-backed identities, mint +
server-registry endpoints — enough to run tier 3 end-to-end in dev/CI and for
small communities, and it _is_ the executable spec of the backend interface (a
real backend reimplements its two endpoints). Explicitly not: accounts UI,
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
