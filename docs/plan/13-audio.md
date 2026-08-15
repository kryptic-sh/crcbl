# Topic 13 — Audio

First-party audio engine (`crcbl-audio`), from scratch like everything else. 3D
audio placement + mixing arrive **early** (before the first sample) because
directional audio is a gameplay pillar, not polish: the spatializer is a
**learnable, deterministic cue grammar** — a skilled player extracts positional
information from sound alone (esports-grade legibility).

## The cue grammar (locked design rules)

Stylized spatialization, deliberately _not_ realistic HRTF. Real HRTF cues are
individually variable and muddy; these are exaggerated, consistent, and
learnable. The rules:

| #   | Direction       | Cue                                                                                                                                                                                                                            |
| --- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Directly ahead  | **Reference signal**: full volume, completely unprocessed. The anchor every other cue is judged against.                                                                                                                       |
| 2   | Right (or left) | Far ear gets **interaural time delay** (sound arrives at left ear slightly late, like real life) + **slightly lower far-ear volume** (ILD). Near ear stays clean.                                                              |
| 3   | Behind          | **Slightly lower volume than front** (real-life shadow) **+ small downward pitch shift** (gameplay cue — front/back is the ambiguity real ears resolve worst, so we make it unmistakable).                                     |
| 4   | Above / below   | **Slight pitch shift**: up-shift for above, down-shift for below. Distinct and symmetric.                                                                                                                                      |
| 5   | Occluded        | **Material-based muffling**: sound behind geometry is lowpass-filtered + attenuated by what's in the way — object type, density, thickness. A closed room muffles everything inside/outside it. Thin wood ≠ concrete, audibly. |

Grammar invariants (what makes it a learnable skill):

- **Deterministic**: same relative position → exactly the same transform, every
  time, every sound. No randomization anywhere in the spatial chain.
- **Continuous**: cues interpolate smoothly across the sphere (front-right-up =
  blend of rules 2+3-inverse+4), so players read angles, not octants.
- **Tunable constants in one struct** (`CueGrammar`): max ITD samples, ILD dB,
  rear dB + rear pitch cents, elevation pitch cents, distance rolloff curve.
  Shipped defaults are the _trained_ grammar; changing them mid-title breaks
  player skill — versioned like a save format.
- Distance: inverse-square-ish attenuation with clamp + optional per-emitter
  curves; doppler is post-MVP (it's a _motion_ cue and must not corrupt the
  positional grammar when it lands).

## Architecture

- **Pure-DSP core, platform seam at the device edge** (same shape as shell/HAL):
  `crcbl-audio` DSP is pure `f32` block processing — runs identically on native
  and wasm, and is unit-testable as plain functions. Device output behind an
  `AudioDevice` seam: `cpal` native (ALSA/PipeWine/ WASAPI/CoreAudio),
  **AudioWorklet** on wasm.
- **Audio thread + lock-free command queue**: game/render side posts commands
  (play, move emitter, set bus gain) into an SPSC ring; the audio callback owns
  all DSP state, never locks, never allocates. Emitter positions stream in as
  snapshots each tick, interpolated per audio block.
- **Internal format**: f32, 48 kHz fixed internal rate, stereo out MVP (the
  grammar is a stereo grammar; surround post-MVP would extend rules, not replace
  them).
- **Voices**: pooled, priority + distance-based stealing, per-voice state =
  {source cursor, resampler, spatial state (ITD delay lines L/R, gains, pitch
  ratio), bus route}.
- **Pitch shift** = resampling ratio (varispeed) — cheap, artifact-free at the
  small cents ranges rules 3/4 use; duration change is irrelevant for cue SFX.
- **Mixer**: bus graph (master ← sfx/music/ui/voice), per-bus gain + soft-knee
  limiter on master. Mix snapshots (menu/gameplay ducking) MVP; effects sends
  (reverb zones, occlusion filters) post-MVP.
- **Assets**: WAV (PCM) + QOA decode from scratch (both are simple, honest
  specs); Vorbis/Opus post-MVP behind the decoder seam. Streaming decode for
  music; SFX fully resident.
- **Server-authoritative fit**: sounds are _events_ (`ServerToClient::Event`) or
  client-derived (own footsteps, UI). Audio rendering is presentation —
  client-side, like graphics; the server never mixes. Default mode: replicated
  events carry `WorldPos` — the spatializer computes relative position from the
  listener (camera/avatar) exactly like the renderer computes view space.
  **Competitive mode** (the `competitive_integrity` gate, topic 31 — one flag
  enables this + the visibility filter together): the server computes the
  grammar per listener and the wire carries only quantized ear-parameters — no
  positions in audio messages; the DSP core is identical either way (it consumes
  parameters, never caring who computed them).
- **Listener** = one entity (client's view); grammar math happens in listener
  space each block.
- **Occlusion (rule 5) rides physics**: per active voice, a client-side
  `crcbl-phys` L0 raycast (listener → emitter, throttled + cached, not per audio
  block) collects hits; colliders carry an **acoustic material**
  (`{density, muffle_cutoff_hz, attenuation_db}` presets: cloth/wood/
  concrete/metal…). Accumulated hits → one-pole/biquad lowpass cutoff + gain per
  voice, parameter-smoothed across blocks (no clicks on corner-peeking). Same
  learnability contract: material → fixed, versioned muffle preset — players
  learn "that's behind wood" vs "behind concrete." Closed rooms need no special
  case: their walls _are_ the occluders. Portal/room-graph acoustics
  (propagation around corners) is post-MVP; the raycast model is the MVP
  grammar.

## Debug tools (topic 7 integration)

- Audio overlay: emitter gizmos with audible-range spheres, per-voice list
  (asset, distance, applied cue values), per-bus meters + limiter activity.
- **Cue inspector**: point at an emitter → shows the exact ITD/ILD/pitch numbers
  being applied; "grammar trainer" toggle plays a reference click on a slow
  orbit around the listener (players + devs calibrate ears).
- `crcbl audio render <scene|script> -o out.wav` (CLI, topic 11): offline render
  of an event script through the full DSP chain — the audio equivalent of
  `screenshot`.

## Testing (topic 12 integration)

- Unit: ITD sample counts, ILD gains, pitch ratios vs closed-form expected
  values across a sphere sweep table; resampler SNR bound; limiter never clips.
- Golden buffers: offline-rendered WAVs hashed/compared with tolerance
  (`--bless` like images); cross-platform identical because DSP is pure f32 with
  fixed order-of-operations.
- Property: grammar continuity (small position delta → bounded output delta — no
  clicks at octant seams); voice steal never leaves dangling delay-line state.
- e2e: sim script emits events → `crcbl audio render` → golden buffer. Runs
  headless everywhere (no audio device needed — device seam!).

## Delivery (interleaved — see ROADMAP)

| Slice                                                                              | Roadmap phase                                            |
| ---------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Device seam + audio thread + mixer/buses + WAV/QOA + voices + **full cue grammar** | P4A (before first sample — breakout bounces pan audibly) |
| Event replication wiring (spatial sounds from server events)                       | P4A (rides P2 machinery)                                 |
| Music streaming + ducking snapshots + cue inspector overlay                        | P10                                                      |
| AudioWorklet wasm output                                                           | P5                                                       |
| **Occlusion (rule 5)**: acoustic materials on colliders, raycast → lowpass chain   | P10 (needs phys BVH + scene materials)                   |
| Reverb zones, portal/room-graph propagation, doppler, surround                     | post-MVP                                                 |

## Exit criteria (MVP)

- Blindfold test passes: a listener with ~15 minutes of grammar-trainer practice
  can call front/back/left/right/above/below of a repeated cue at ≥90% accuracy
  — the esports-legibility bar, measured, in the doc.
- Grammar continuity property suite green (no seams/clicks on orbiting
  emitters).
- towers plays creep/tower audio spatially in co-op (browser + native, same
  grammar — wasm AudioWorklet path verified).
- Occlusion audible and material-distinct: the same cue behind a thin wall vs
  heavy wall vs open air is identifiable in the grammar trainer; walking into a
  closed room audibly muffles the outside world.
- Golden-buffer audio e2e in CI for every sample that emits sound.

## Corrections (design review, 2026-07-27)

- **Fractional delay lines are required for ITD** (the grammar's own failure
  mode): as an emitter moves, ITD length modulates continuously; changing an
  integer delay tap produces clicks, and delay modulation _is_ pitch shift —
  corrupting the very pitch cues rules 3/4 depend on. Named machinery:
  **fractional delay with linear/Lagrange interpolation**, per-block parameter
  smoothing, and **crossfaded dual delay lines** for large jumps and left↔right
  ear swaps. New property: an orbiting emitter produces no unintended pitch
  glide beyond a stated cents bound.
- **"Identical across platforms because pure f32" is only true without libm.**
  Biquad coefficients (`cos`, `exp`), resampler windows (`sin`) and cents→ratio
  (`powf`) all hit platform libm, which differs across glibc/musl/macOS/wasm —
  golden buffers would mismatch between dev and CI. **Fix**: own polynomial
  approximations / LUTs for every transcendental in the DSP path, a CI `deny` on
  std float transcendentals inside `crcbl-audio` (the same pattern as the
  `std::fs` deny), and no `mul_add`/FMA contraction.

  **The `std::fs` deny it cites does not exist, 2026-08-15.** There is no
  `clippy.toml` in the workspace and no `disallowed_methods` list anywhere; the
  workspace's clippy configuration is `clippy::all` at `warn` and nothing else,
  and no CI step greps for `std::fs` either. Whoever builds the transcendental
  deny is building the first one of its kind rather than copying a pattern, so
  the mechanism is part of that slice: a `clippy.toml` with `disallowed-methods`
  is the shape, and it has to be shown to fail before it is trusted.

## Correction (2026-08-09)

- **The bus graph is specified here and does not exist.** The architecture
  section calls for `master ← sfx/music/ui/voice`, per-bus gain and a soft-knee
  limiter on master, and the delivery table puts buses in **P4A**, which the
  ROADMAP marks done. `crates/crcbl-audio` has no bus type and no limiter —
  `Mixer` is voices and nothing above them. Mix snapshots and ducking are
  scheduled at P10 and depend on the buses that were never built, so P10
  inherits both.
- **The transcendental policy conflicts with [05-physics.md](05-physics.md).**
  This document requires own polynomial approximations plus a CI deny; topic 5's
  correction requires the `libm` crate. Neither exists. See topic 5's correction
  for the full note; the decision is one decision and belongs in one place.
- **Golden buffers are an exit criterion with no instances.** "Golden-buffer
  audio e2e in CI for every sample that emits sound" is stated above; asteroids
  and horde both synthesise their cues deterministically from fixed seeds — so a
  golden is possible — and neither has one. `docs/backlog.md` carries it as a
  coverage gap.
- **There is still no listener.** `spatial::compute_cue` takes the listener
  position on every call, so four samples spell their audio entry point three
  different ways. This document's architecture already decided the answer —
  "**Listener** = one entity (client's view); grammar math happens in listener
  space each block" — so the gap is unbuilt work rather than an open design
  question, and the backlog entry that frames it as a design question is wrong.

  **Closed, 2026-08-15.** `crcbl_audio::spatial` has a `Listener`, with
  `Listener::ORIGIN` as the value a mixer starts at, and the `Mixer` owns one:
  `Mixer::set_listener` is where a frame says where the ears are, and cues are
  placed against it rather than against a position each caller passes in. Built
  as this bullet argued — the architecture's answer, not a new one.
