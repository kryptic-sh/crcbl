# Topic 32 — Voice (VOIP)

In-game voice: low latency, speech-quality audio, two broadcast modes —
**team/direct** (roster-routed radio) and **world** (proximity voice through the
cue grammar). World voice obeys the `competitive_integrity` gate exactly like
footsteps: when the gate is on, **no positional data reaches the wire**.
FPS-era, with breach; the capture seam is useful earlier.

## Modes

| Mode                  | Routing                                                               | Presentation                                                                                | Leak profile                                                                                         |
| --------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| **Team / direct**     | Roster membership (team id, party, friend list) — server-resolved     | Non-positional "radio": full volume, unprocessed, optional radio-filter flavor              | None — a roster route carries no world data                                                          |
| **World / proximity** | Server-side audibility (range + occlusion) — same test as any emitter | Spatialized through the cue grammar (13): ITD/ILD, rear/elevation pitch, material occlusion | Gate off: positional like other events. **Gate on: server-computed ear-params only, no coordinates** |

- Both modes can be active simultaneously (talk to team while a nearby enemy
  hears your world voice — the tension that makes proximity voice fun).
- **Gate-on bonus**: the server already decides who can hear you, so it only
  relays to those listeners. World voice under the gate is both _safer_ and
  _cheaper_ on bandwidth than the naive broadcast.
- Mode selection is input, not config: separate actions (topic 19) `voice_team`
  and `voice_world`, push-to-talk by default, open-mic (VAD) as a setting.
  Hold-pattern semantics come free from the action layer.

## Capture (the seam topic 13 doesn't have yet)

- **`AudioCapture` seam** beside `AudioDevice`: enumerate inputs, select device,
  open at 48 kHz mono (engine-internal rate — no resampling), 20 ms frames.
  `cpal` native; `getUserMedia` + AudioWorklet on wasm (permission is a
  user-gesture flow — surfaced through `ShellCaps`-style capability reporting so
  UI degrades honestly).
- Pre-encode chain: DC block → noise gate → VAD (hangover-smoothed) → optional
  AGC. Kept deliberately minimal.
- Settings UI (topic 14, P10 screen): device pick, input level meter, mic test
  with **loopback self-monitor**, PTT/VAD toggle, VAD threshold.

## Codec + quality

- **Opus** at 24–32 kbps VBR mono, wideband/fullband speech — the honest choice:
  it _is_ the open standard for this (royalty-free, RFC 6716) and writing a
  competitive speech codec from scratch is a multi-year DSP project with a worse
  result. **Sanctioned exception behind a `VoiceCodec` seam**, same policy as
  wasmtime/RustCrypto/quinn: the protocol, buffering, routing, and
  spatialization are ours; the codec is not. (Browsers ship Opus natively — the
  wasm build can use WebCodecs instead of shipping the encoder, decided at
  implementation time.)
- 20 ms frames = 50 packets/s/speaker; **Opus in-band FEC + PLC** carries lossy
  links without retransmits.

## Latency budget (measured, not hoped)

| Stage                  | Target                                |
| ---------------------- | ------------------------------------- |
| Capture buffer         | 20 ms                                 |
| Encode                 | ~5 ms                                 |
| Network (½ RTT)        | 10–40 ms                              |
| Adaptive jitter buffer | 20–60 ms                              |
| Decode + mix + output  | ~15 ms                                |
| **Mouth-to-ear total** | **< 120 ms typical, < 150 ms budget** |

- **Adaptive jitter buffer**: depth tracks measured jitter, shrinks in calm
  conditions; PLC covers gaps; late frames drop rather than stall.
- Voice rides the **unreliable-sequenced channel** (23) — never reliable, never
  retransmitted (a resent voice frame is always too late).

## Server relay (never P2P)

- All voice flows client → server → listeners. P2P would leak IPs and positions
  and would bypass every routing/moderation guarantee — rejected.
- Server enforces routing: team packets reach only that roster; world packets
  reach only audibly-plausible listeners (range + occlusion test, sharing the
  vis/occlusion ray budget from 31).
- **Bandwidth honesty**: relay cost is O(speakers × listeners). Mitigated by
  audibility culling, per-speaker rate caps, and a hard cap on concurrent
  relayed speakers (excess = queued/dropped with a UI indicator).
- Voice packets are hostile input: size caps, per-client rate limits, decode
  hardening + fuzzing (23) apply unchanged.

## Moderation, privacy, casting (stated positions)

- **Mute/volume per player**, keyed on PlayerId (27): client-side mute for
  preference; **server-enforced mute/block** for reports and bans (a blocked
  speaker's packets are never relayed — the only mute a modified client can't
  undo).
- **Privacy default: voice is NOT recorded into replays** (22). Opt-in per
  server for tournament/casting use, with the consent implication stated in the
  config docs — an engine that silently archives voice would be hostile.
- **Casters/observers** may subscribe to team channels only when the server
  grants the observer role (27 claims) — a tournament setting, never a default.

## Under the competitive gate (31)

- World voice becomes **server-spatialized**: the wire carries
  `{speaker_id, frame, quantized ear-params}` — no coordinates, ear parameters
  quantized to the same JND floors as footsteps. The leak is capped at the
  perceptual cone; a cheat learns what ears learn.
- Team voice is unaffected (it never carried position).
- The audio-path schema assert from 31 covers voice packets automatically —
  voice is part of the all-channel leak property, not an exception to it.
- The leak checklist gains: don't render a "who's talking" indicator
  positionally for world voice under the gate (a floating name over a wall is a
  wallhack with extra steps) — speaker UI is a nameplate only when the speaker
  is already visible.

## Testing (topic 12)

- **Latency harness**: timestamped loopback through the full chain asserts the
  budget table; regressions fail CI (headless — no devices needed via the
  capture seam's null input).
- Codec roundtrip goldens (SNR bound); jitter-buffer property tests
  (reorder/loss/burst patterns → bounded artifacts, adaptive depth converges).
- **Routing property**: team packets never reach non-members; world packets
  never reach listeners outside the audibility test (fuzzed rosters +
  positions).
- **Gate leak property**: with the gate on, no voice packet contains coordinates
  (schema assert), ear-params meet quantization floors.
- Moderation: server-enforced mute/block verified at the relay, not the client.

## Delivery

| Slice                                                            | Phase                                           |
| ---------------------------------------------------------------- | ----------------------------------------------- |
| `AudioCapture` seam + device settings + loopback self-monitor    | can land any time after P10 (useful standalone) |
| Codec seam + Opus + jitter buffer + PLC/FEC + latency harness    | FPS-era                                         |
| Team/direct routing + PTT/VAD actions + mixer voice bus/ducking  | FPS-era                                         |
| World proximity voice through the cue grammar                    | FPS-era                                         |
| Gate integration (server-spatialized params, audibility culling) | FPS-era (with 31)                               |
| Moderation (mute/block/volume), casting permissions              | FPS-era                                         |
| Replay opt-in recording                                          | post-breach                                     |

## Risks

- **Echo/feedback**: full acoustic echo cancellation is research-grade DSP and
  is **out of scope** — the engine ships a noise gate + ducking and recommends
  headsets, stated plainly rather than shipping a bad AEC. Platform/browser AEC
  is used where the OS offers it for free.
- **Latency creep**: every buffer is a temptation; the budget table is a test,
  so growth fails CI rather than shipping.
- **Relay bandwidth at scale**: audibility culling + speaker caps; numbers
  recorded from breach matches.
- **Codec dependency**: seam-isolated; a from-scratch codec remains possible as
  its own exercise, and the browser path may not ship one at all.

## Correction (design review, 2026-07-27)

**WebCodecs audio _encode_ support is uneven** across Firefox/Safari, so the
browser path cannot assume it. Corrected: ship **libopus compiled to wasm as the
baseline** for browser encode/decode, with WebCodecs used opportunistically
where available (a capability check, not a requirement).
