# Audit — P2b resume credentials (`crcbl-net`, `-server`, `-client`)

**Last updated:** 2026-07-29, covering `ed0d9b2` and `97c7085`. **Method:** the
eight gates and the harnesses from the ROADMAP's "How work proceeds" section,
plus throwaway probe binaries. Every finding below was produced by running
something, not by reading.

**Verdict:** **no blocking defects.** Clippy
`--all-targets --all-features -D warnings` clean, **869 tests** (up from 861),
and all four findings from the previous round are fixed — the blocking one
properly, not narrowly. What remains are three hardening gaps and one convention
break, none of which stops the slice.

**Resolved this round** (detail pruned):

- The session-hijack hole is closed. `ResumeToken([u8; 32])` is filled from the
  OS CSPRNG, is separate from the routing `SessionId`, compares in constant
  time, and redacts itself in `Debug`. `Server::try_new` returns the entropy
  error rather than falling back to a predictable credential — the right call,
  and not the obvious one.
- `ProtocolCompatibility` makes `engine_build_id` / `schema_hash` injectable via
  `new_with_compatibility`, and `DEFAULT` now says in as many words that its
  zeroes "provide no engine or schema protection". Protocol version went 1 → 2,
  which is correct: `Hello` and `Accept` both changed size incompatibly.
- `from_snapshot` is the checked constructor again and `from_trusted_snapshot`
  is the explicit opt-out — the safe path got the short name, and the opt-out
  has real callers instead of being dead code.
- `Instant::now()` is gone from the tick loop; `Server` stores the injected
  `now: Duration`, and `injected_time_expires_reconnect_without_sleep` tests
  grace expiry without sleeping.

Verified independently — three servers, then four wrong tokens against a
disconnected session, each on its own tick:

```text
issued session_id=SessionId(1) token[0..4]=[db, a7, af, 04]
issued session_id=SessionId(1) token[0..4]=[b3, 56, 86, 2a]
issued session_id=SessionId(1) token[0..4]=[e2, 9d, 9f, bc]
guess all-zero:           Reject { code: 4 } / state=Reconnecting
guess all-one:            Reject { code: 4 } / state=Reconnecting
guess off-by-one-byte:    Reject { code: 4 } / state=Reconnecting
guess first-byte-flipped: Reject { code: 4 } / state=Reconnecting
real token:               Accept { resume_token: ResumeToken([REDACTED]) } / state=Connected
```

Single-bit variants at both ends of the token are rejected, and the redaction
holds in the `Accept` that a log line would actually print.

---

## Open findings

### 1. The resume token is never rotated, so one capture owns the session forever

`rotate_session` is reachable from exactly one place — a fresh `Hello` arriving
after the reconnect grace period expired. A _successful_ resume issues no new
credential: the `Accept` echoes `self.resume_token` unchanged, and the session
becomes resumable again with the same bytes.

Probed — capture the token once, then replay it across a second disconnect:

```text
ORIGINAL token replayed after 2nd disconnect -> ACCEPTED; same token reissued = true
```

The transport this rides on is unauthenticated and datagram-shaped, so anyone
who observes one handshake — once — can resume that session at every future
disconnect, indefinitely. Rotation on expiry closes the window after the session
dies; it does nothing for a session that keeps living.

The fix is nearly free because the plumbing already exists: `Accept` carries a
`resume_token` field, and `Client` overwrites its stored token from whatever
arrives. Generating a fresh token on each successful `try_reconnect` and sending
that instead makes a captured token single-use. The test to add is the probe
above inverted — assert the second `Accept`'s token differs from the first, and
that the first no longer resumes.

### 2. `ResumeToken` makes the timing-unsafe comparison the ergonomic one

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ResumeToken(pub [u8; 32]);
```

`constant_time_eq` exists, is correct, and — unlike the last round's helper — is
genuinely called at both comparison sites in `crcbl-server`. But the derived
`PartialEq` means `a == b` also compiles, short-circuits on the first differing
byte, and is what anyone will reach for first; the `pub` field means
`a.0 == b.0` works too. The safe path is opt-in and longer, which is the same
shape as last round's `from_snapshot` naming problem, resolved there and
reintroduced here.

**Fix:** hand-write `PartialEq` as the constant-time comparison rather than
deriving it. Then `==` is safe, `constant_time_eq` becomes redundant, and there
is no unsafe spelling left to pick by accident. Making the field private with a
constructor closes `.0` too, and would have prevented my probe from forging
tokens — which is a point in its favour, not against.

### 3. "Production embeddings must use explicit identifiers" is enforced by nothing

`ProtocolCompatibility::DEFAULT` documents its zeroes as placeholders, which was
the honest option and I asked for it. But `Server::new` and `Client::new` still
select `DEFAULT`, so the shipped default path compares `0` to `0`, and the only
thing standing between that and production is a doc comment.

This repository's own recurring lesson applies: a check nobody runs is not a
check, and a rule nobody enforces is not a rule. A `debug_assert!` on non-zero
identifiers at `Server::new`, or a test asserting the sim app configures real
ones, converts the comment into something CI can fail on. Either is a few lines,
and the placeholder can stay until P2c supplies real values.

### 4. `getrandom` is the one dependency that skips `[workspace.dependencies]`

```toml
# crates/crcbl-server/Cargo.toml
getrandom = "0.4.3"
```

Every other dependency in the workspace — internal and external — is
`{ workspace = true }`. The root manifest says why, and the reason is not
cosmetic:

> Internal crates are pinned here too, so a member never spells out a path more
> than once and the dependency direction is readable from one place. The
> redundant-looking `version` is load-bearing: `cargo deny check bans` treats a
> bare `{ path = … }` as a wildcard dependency and fails the build.

A second crate needing entropy will now pin its own version, and the two can
drift. Move it to `[workspace.dependencies]` with the rest.

---

## What this round got right

- **The blocking finding was fixed at the root, not at the symptom.** The cheap
  patch was to randomise `SessionId`. Instead the commit split the _routing
  identifier_ from the _credential_, which is the distinction the bug was
  hiding: `SessionId` is now documented as "not a credential" and stays small
  and predictable, while `ResumeToken` carries 256 bits of OS entropy. Three
  further properties came along that I did not ask for — constant-time compare,
  `Debug` redaction, and a fallible constructor that refuses to invent a
  credential when the CSPRNG is unavailable.
- **`try_new` is the right shape.** `Server::new` keeps the ergonomic signature
  and panics with a message naming the cause; `try_new` /
  `try_new_with_compatibility` give an embedder the error. Nothing silently
  degrades to a weak token, which is the failure mode that would have quietly
  undone the whole fix.
- **The naming fix was applied in the direction that makes the default safe.**
  `from_untrusted_snapshot` did not simply gain a caller — it was inverted, so
  `from_snapshot` is checked and the opt-out has to be spelled out. That is the
  harder edit (it touched ~40 call sites in tests) and the right one.
- **The version bump was noticed.** `Hello`'s token field went 8 → 32 bytes and
  `Accept` grew by 32; `protocol_version` went to 2 and
  `protocol_version_two_rejects_legacy_version_one` pins it. An old client now
  gets a clean `0x01` rejection instead of a decode error, which is the
  difference between a diagnosable failure and a mystery.
- **`rotating_session_clears_baselines_and_acks` asserts the right things** —
  not just that the id changed, but that `last_acked_tick` is `None` and the
  baseline store is empty. A rotation that kept the old baselines would have
  been a state-leak across sessions, and the test would catch it.

---

## The pattern worth carrying forward

Last round's summary was "a check whose inputs are constants is not a check".
Both instances are now fixed, and the fix for the first one is genuinely good
security engineering rather than a minimal patch.

The residue is a narrower version of a habit this codebase keeps rediscovering:
**when there are two ways to do something and one is wrong, the wrong one keeps
getting the shorter name.** `from_snapshot` versus `from_untrusted_snapshot`
last round; `==` versus `constant_time_eq` this round. Both times the safe
behaviour existed and was correct — it just was not the default spelling, so
nothing stops the next caller from picking the other one. Deriving `PartialEq`
is a decision, and here it silently reintroduced the thing `constant_time_eq`
was added to prevent.

Finding 1 is different in kind and is the one worth doing next: rotation on
expiry protects a dead session, and the live one is the valuable one.

Trajectory across rounds: blocking CI failures → soundness/UB → assertion
strength → observability of state → observability of failure → adequacy of the
checks → lifetime of the credentials those checks accept.
