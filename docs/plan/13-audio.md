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
  and wasm, and is unit-testable as plain functions. Device output behind a
  device seam: `cpal` native (ALSA/PipeWire/WASAPI/CoreAudio), **AudioWorklet**
  on wasm. **Built, and the seam's name is `AudioStream`, not `AudioDevice`** —
  `AudioStream::open` takes an `AudioSource` and, on `wasm32`, installs it as
  the pull target `crcbl_audio::web`'s worklet drives; a null backend keeps the
  headless CI path device-free. Grep for `AudioDevice` and you find nothing.
- **Audio thread + lock-free command queue**: game/render side posts commands
  (play, move emitter, set bus gain) into an SPSC ring; the audio callback owns
  all DSP state, never locks, never allocates. Emitter positions stream in as
  snapshots each tick, interpolated per audio block.
- **Internal format**: f32, 48 kHz fixed internal rate, stereo out MVP (the
  grammar is a stereo grammar; surround post-MVP would extend rules, not replace
  them).
- **Voices**: pooled, priority + distance-based stealing, per-voice state =
  {source cursor, resampler, spatial state (ITD delay lines L/R, gains, pitch
  ratio), bus route}. **The per-voice state is built and the pool is not**
  (2026-08-27): a `Voice` carries its cursor, varispeed pitch, per-channel gains
  and its L/R fractional delay lines, but `Mixer` holds voices in a plain
  growable `Vec` behind a `Mutex` — no capacity, no priority, no stealing, and
  therefore nothing that bounds what a game can start playing. There is a
  release list, so a stopped voice fades over one block rather than clicking.
  Whoever adds the pool inherits the invariant the release list already keeps:
  ids are monotonic and never reused, so a stale `VoiceId` can never name a
  later voice, and a stealing scheme that recycles slots must not break that.
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
  space each block. **Built**: `crcbl_audio::spatial::Listener`, with
  `Listener::ORIGIN` as the value a mixer starts at; the `Mixer` owns one and
  `Mixer::set_listener` is where a frame says where the ears are, so a cue is
  placed against the mixer's listener rather than against a position each caller
  passes in. (`spatial::compute_cue` still takes a listener position as an
  argument, and that is the layer below — call it directly and you are back to
  four samples spelling the entry point four ways.)
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
  `screenshot`. **Not built** (2026-08-27): `crcbl-cli`'s parser accepts `new`,
  `run`, `build`, `screenshot`, `replay`, `crpix`, `lod`, `import`, `bench`,
  `sim` and `settings`, and `audio` is not among them — an `audio` invocation
  exits 2 as an unrecognized command. It is the missing half of the e2e below,
  which is written as "sim script → `crcbl audio render` → golden buffer", and
  its input is the part to think about first: a scene is the `.scn/` RON
  directory `crcbl-scene` has not built, and `docs/backlog.md` records
  `crcbl sim --input script.ron` refused for the same reason — nothing in the
  workspace reads RON.

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

**What of this list actually runs** (2026-08-27), because the section reads as
description and is a plan: `crates/crcbl-audio/tests/spatial_chain.rs` covers
the chain end to end — centre is symmetric, right pans right, an event stream
hashes the same twice and differently reversed — and `synth`'s in-crate tests
cover the one golden buffer. **Not written**: the sphere sweep against
closed-form values, the resampler SNR bound, the limiter assertion (there is no
limiter), the continuity property, the voice-steal delay-line property (there is
no stealing either), and the e2e (there is no `crcbl audio render`). The
headless claim is the one thing this list can already prove — the null output
backend makes every test above run with no device.

## Delivery (interleaved — see ROADMAP)

| Slice                                                                                                                                                                                                                                                                                                                                                                                             | Roadmap phase                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| Device seam + audio thread + mixer/buses + WAV/QOA + voices + **full cue grammar** — **built except the buses**: `AudioStream`, `Mixer`, `wav.rs`, `qoa.rs`, `spatial.rs`. The buses are the one hole and they have their own correction below.                                                                                                                                                   | P4A (before first sample — breakout bounces pan audibly) |
| Event replication wiring (spatial sounds from server events)                                                                                                                                                                                                                                                                                                                                      | P4A (rides P2 machinery)                                 |
| Music streaming + ducking snapshots + cue inspector overlay                                                                                                                                                                                                                                                                                                                                       | P10                                                      |
| AudioWorklet wasm output — **built**: `crates/crcbl-audio/src/web.rs`. Read its header first: the worklet and the wasm instance are the same thread, so the SAB ring an AudioWorklet feed would normally use is not what this does.                                                                                                                                                               | P5                                                       |
| **Occlusion (rule 5)**: acoustic materials on colliders, raycast → lowpass chain — **unbuilt, and its dependency is now half met**: `crcbl-phys` has the ray query (`cast_ray`), so what is owed is the acoustic-material presets on colliders and a per-voice filter. There is no biquad or one-pole in the voice path at all — the only lowpass in `crcbl-audio` is inside the noise generator. | P10 (needs phys ray queries + scene materials)           |
| Reverb zones, portal/room-graph propagation, doppler, surround                                                                                                                                                                                                                                                                                                                                    | post-MVP                                                 |

## Exit criteria (MVP)

- Blindfold test passes: a listener with ~15 minutes of grammar-trainer practice
  can call front/back/left/right/above/below of a repeated cue at ≥90% accuracy
  — the esports-legibility bar, measured, in the doc.
- Grammar continuity property suite green (no seams/clicks on orbiting
  emitters).
- towers plays creep/tower audio spatially in co-op (browser + native, same
  grammar — wasm AudioWorklet path verified). **There is no towers app** — the
  `apps/` tree has fourteen samples and towers is not one of them, so this
  criterion has no way to be met and no date; the AudioWorklet half of it is
  separately provable and built.
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

  **One of the three is built, and the omission is the one this bullet is
  about** (2026-08-27). `crcbl-audio`'s `mixer` has a per-channel `DelayLine`
  answering a linearly interpolated fractional delay, capacity one past the
  longest ITD it will serve so both taps stay in-buffer. What it does **not**
  have is any smoothing or crossfade: `Mixer::set_mix` — documented as "re-aim a
  playing voice", the moving-emitter case exactly — reaches `Voice::apply_mix`,
  which assigns `itd_samples` straight into the voice with a clamp and nothing
  else. So a re-aimed voice steps its delay length in one block, which is the
  click and the glide described above. Neither the cents-bound property nor a
  continuity property exists; the spatial tests assert symmetry, panning
  direction and determinism, not smoothness.

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
  inherits both. **Still true on 2026-08-27** — no `Bus`, no `Limiter`, nothing
  named either, anywhere under `crates/crcbl-audio/src`. It has a third debtor
  now: [32-voip.md](32-voip.md) schedules "mixer voice bus/ducking" with team
  voice, and that bus is this bus.
- **The transcendental policy conflicts with [05-physics.md](05-physics.md).**
  This document requires own polynomial approximations plus a CI deny; topic 5's
  correction requires the `libm` crate. Neither exists. See topic 5's correction
  for the full note; the decision is one decision and belongs in one place.
- **Golden buffers exist now, but not at the level the exit criterion names**
  (revised 2026-08-27; this bullet said "no instances"). There is exactly one:
  `crates/crcbl-audio/tests/burst-reference.wav`, asteroids' explosion, written
  and read by this crate's own WAV codec and compared frame by frame against the
  generator in `crcbl-audio`'s `synth` tests. What is still missing is the
  criterion as written — "for every sample that emits sound". `apps/asteroids`
  and `apps/horde` synthesise their cues deterministically from fixed seeds, so
  a golden is possible for each, and neither has one; only `apps/breakout` even
  mentions audio in its test target. `docs/backlog.md` carries the gap.

  **Read that golden's own doc comment before writing the next one.** It started
  as a digest of every sample's `f32::to_bits` and CI failed it on macOS _and_
  on Windows the first time it ran — Windows being `x86_64` like the Linux
  runner is what rules out an architecture cause and pins it on libm, exactly as
  the transcendental correction above predicts. It now pins the **waveform** at
  a tolerance, plus total energy separately, because a per-sample bound is blind
  to a small coherent drift spread over a whole buffer. A sample-level golden
  that pins bytes will fail the same way.

- ~~**There is still no listener.**~~ **Closed 2026-08-15, verified again
  2026-08-27** — `Listener`, `Listener::ORIGIN` and `Mixer::set_listener` all
  ship; see the architecture bullet above. Kept as a line rather than deleted
  because of _how_ it closed: the answer was already written in this document's
  architecture section, so the work was unbuilt work and not an open design
  question, and the backlog entry that framed it as a design question was wrong.
  That is the failure mode to watch for in the bullets around it.
