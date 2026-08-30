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
- **Mixer**: per-bus gain + soft-knee limiter on master. Mix snapshots
  (menu/gameplay ducking) MVP; effects sends (reverb zones, occlusion filters)
  post-MVP. **The bus set is decided and superseded this bullet's
  `master ← sfx/music/ui/voice` sketch** — six fixed buses, the multiply order
  that resolves them, and the `[engine.audio]` keys that drive them are under
  "Buses and volume" below, along with the refusal of an arbitrary routing
  graph. None of it is built.
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

## Buses and volume (LOCKED 2026-08-27)

### Two consequences of the gain chain

Neither of these was chosen; both fall out of the decision below:

- **`Mixer::route_gain` returns unity for `Bus::Master`.** The master is a bus
  like the others _and_ the stage every voice passes through, so a voice routed
  to it would otherwise take its gain twice — a master at a half turning exactly
  those voices down to a quarter.
  `a_voice_on_the_master_bus_is_not_attenuated_twice` is the guard.
- **The gains are read once per block, not once per voice.** A gain that moved
  between two voices of one block would put a step inside the buffer that
  neither voice asked for.

The order contract is asserted by `the_gain_chain_is_voice_then_bus_then_master`
with three gains whose products differ under regrouping — and the test checks
that property of the three gains first, so a later edit cannot quietly pick
three that associate and leave the assertion unable to fail.

**What is not built**: `AudioEvent` still carries no bus, which is the wire
break the touch list below prices.

### The decision: a small fixed set of buses

**Six buses, fixed, named in the engine: `master`, `music`, `sfx`, `ui`,
`voice`, `ambience`.** Each is one linear gain. A voice is routed to exactly one
bus **at spawn**, and its route does not change for its lifetime — a sound that
would need to move between buses is two sounds.

This supersedes the architecture section's `master ← sfx/music/ui/voice` sketch
by widening it, and the two additions are the ones a game notices immediately:
`ambience` because a wind loop that ducks with the gunfire is a bug players
report, and `voice` because [32-voip.md](32-voip.md) schedules team voice
against this bus and team voice is the one thing a player must be able to
silence independently of everything else.

### Final gain is three multiplies, in a stated order

```text
sample × voice_gain × bus_gain × master_gain
```

**The order is part of the contract, not an implementation detail**, and this
crate is the one place in the workspace where that sentence is load-bearing
rather than pedantic. Floating-point multiplication is not associative:
`(a × b) × c` and `a × (b × c)` differ in the last bits, and the golden-buffer
discipline in the Testing section compares rendered audio against a checked-in
reference. A reordering that looks like a refactor moves every sample of every
golden. So: left to right, voice first, master last, and a test asserts the
order rather than the result — the way to check it is to give the three gains
values whose products differ under regrouping and compare against the sequence
spelled out, not against a constant somebody once observed.

The same rule is why the sum is where it is. Voices are summed into the buffer
in `AudioSource::fill` and the sum is clamped once; per-bus gain applies to a
voice before it enters that sum, so a bus is a **gain on the way in**, not a
second buffer that is mixed down afterwards. That keeps the buffer count at one
and keeps the arithmetic order stateable in a single line, which the paragraph
above needs.

`master` is a bus like the others and is also the one every voice passes
through, which is what makes it the third multiply rather than a fourth concept.

### Considered and declined: an arbitrary routing graph

**A user-defined bus graph with sends, returns and per-bus effect chains is
refused.** That is a mixer product — it is what a DAW is — and nothing in this
workspace has asked for one. A game asks "can the player turn the music down",
"does dialogue duck the music", and "can I silence team voice"; all three are
answered by a fixed set of named gains, and none of them needs an arbitrary
topology.

What the refusal costs and what it does not: ducking (the mix snapshots this
document schedules at P10) is a _modulation of bus gains over time_, not a
routing change, so it is unaffected. Reverb sends genuinely would need the graph
— and reverb zones are already post-MVP here, so the graph is refused for
exactly as long as reverb is. Revisit only with a concrete effect that has to be
shared across several buses at once; do not revisit because the fixed set feels
inflexible.

A second refusal, smaller and easier to slip into: **the bus set is not
game-extensible.** A game that wants a seventh category uses the closest of the
six. The set is small so that a settings screen can lay it out without
scrolling, so that `[engine.audio]` has a fixed key list, and so that a game's
choice of bus names cannot become a compatibility surface.

### The persisted keys, and why their clamp chain is shorter

One `[engine.audio]` key per bus, each a linear gain in `[0, 1]`:

| Key               | Domain          |
| ----------------- | --------------- |
| `master_volume`   | linear `[0, 1]` |
| `music_volume`    | linear `[0, 1]` |
| `sfx_volume`      | linear `[0, 1]` |
| `ui_volume`       | linear `[0, 1]` |
| `voice_volume`    | linear `[0, 1]` |
| `ambience_volume` | linear `[0, 1]` |

Storage, spelling and the absent-key rule are
[14-persistence.md](14-persistence.md)'s; `master_volume` is adopted rather than
renamed precisely because `crcbl-store`'s own example already writes it.

**The clamp chain here is two layers, not four, and the missing layers are
missing for a reason rather than unbuilt.** A video key resolves through camera
stack → `[engine.video]` → programmatic override → device capability. For an
audio volume:

- There is **no per-camera layer**, because there is no per-camera audio. There
  is exactly one listener — `Mixer::set_listener` — and one mix.
- There is **no device-capability layer**, because no audio device removes the
  ability to multiply a sample by a scalar. The DSP core is pure `f32` block
  processing that runs identically on native and wasm, which is this document's
  own architecture rule. [39-capabilities.md](39-capabilities.md) says the same
  thing from the other side.

What remains is the player's file and the game's programmatic control, and the
resolution is a plain multiply: the file sets the bus gain, and a game that
ducks does so by scaling on top of it. Note that this makes an audio key
**unlike** a video key in one further respect worth stating so nobody
generalises wrongly: `[engine.video]` may only clamp downward, whereas an
`[engine.audio]` key _is_ the value. There is nothing above it for it to clamp
against.

### What this touches when it is built

- `crates/crcbl-audio/src/mixer.rs` — the bus set and its gains, the routing
  field on `Voice`, and the multiply in `Voice::mix_block`. `Mixer::play` is
  where a route is fixed, because that is where a voice is handed over.
- The `Voice` construction path — `Voice::new`, `Voice::from_shared` and
  `SoundBank::create_voice`. A bus has to be chosen somewhere, and a default of
  `sfx` for a voice built without one is what keeps every existing caller
  compiling and audible.
- `crates/crcbl-audio/src/event.rs` — **only if a bus travels with an event.**
  `AudioEvent` is a 28-byte little-endian wire format with `sound_id`,
  `position`, `range`, `volume` and `pitch`, and adding a bus makes it 29 or 32
  and is a wire break. It is worth doing rather than deriving the bus from the
  sound id, because the same sound legitimately belongs to different buses in
  different contexts — but it is a versioned wire change and `WIRE_SIZE` plus
  both round-trip tests move with it.

## The rest of the audio settings (LOCKED 2026-08-27)

The bus gains are the keys players reach for first; they are not the whole
`[engine.audio]` section. Named here on the same terms as the video catalogue —
key, domain, and what implements it — so the section does not have to be renamed
later.

| Key                  | Domain                                           | Today                                                                                                                                                                                                                                                                      |
| -------------------- | ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `output_device`      | device **name**; absent means the system default | **Nothing.** `AudioStream::open` takes the default device; nothing enumerates devices and nothing selects one. Resolved by name with a fallback to default, never by index, for the reason [15-windowing.md](15-windowing.md) gives about monitors.                        |
| `speaker_config`     | `mono` \| `stereo`                               | **Nothing, and `stereo` is the only rung the DSP has.** The internal format is fixed stereo and the cue grammar is a stereo grammar; surround is post-MVP in the delivery table below, so it is deliberately absent from this domain rather than listed and unimplemented. |
| `dynamic_range`      | `full` \| `night`                                | **Nothing.** There is no limiter or compressor in the crate at all — the 2026-08-09 correction below records the missing master limiter, and night mode is that limiter driven harder.                                                                                     |
| `mute_on_focus_loss` | `true` \| `false`                                | **Nothing reads it**, though the input side exists: the shell already emits `ShellEvent::Focus`.                                                                                                                                                                           |

### A mono option is an accessibility feature, not a downgrade

`speaker_config = "mono"` sums the stereo mix to a single channel sent to both
ears. It looks like throwing information away, and for most players it is — but
it is the difference between playing and not playing for a player with hearing
in one ear, and the same for anyone on a single speaker.

It deserves saying in this document specifically, because this engine's audio is
a **gameplay pillar**: the cue grammar exists so a skilled player extracts
position from sound, and the horizontal half of that grammar — rules 2's ITD and
ILD — collapses entirely in mono. Front/back and elevation do not: those are
rules 3 and 4, and they are _pitch_ cues, which survive the summing intact. That
is not a coincidence, it is the grammar's design paying off in a case it was not
drawn for — the pitch cues were chosen because real ears resolve front/back
worst, and it turns out they are also the cues that survive with one ear.

So the mono path is not "stereo with the pan removed and the player on their
own". It is a supported configuration in which part of the grammar is available
and part is not, the trainer should say which, and the sphere-sweep tests this
document owes should cover it. Whoever builds it: sum to mono **after** the
spatial chain, not by skipping it, or the ITD delay lines stop being exercised
and the elevation cues go with them.

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
| Device seam + audio thread + mixer/buses + WAV/QOA + voices + **full cue grammar**                                                                                                                                                                                                                                                                                                                | P4A (before first sample — breakout bounces pan audibly) |
| Event replication wiring (spatial sounds from server events)                                                                                                                                                                                                                                                                                                                                      | P4A (rides P2 machinery)                                 |
| Music streaming + ducking snapshots + cue inspector overlay                                                                                                                                                                                                                                                                                                                                       | P10                                                      |
| AudioWorklet wasm output — read `crates/crcbl-audio/src/web.rs`'s header first: the worklet and the wasm instance are the same thread, so the SAB ring an AudioWorklet feed would normally use is not what this does.                                                                                                                                                                             | P5                                                       |
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

- **The master limiter is specified here and does not exist.** The architecture
  section calls for a soft-knee limiter on master, and the delivery table puts
  it in **P4A**, which the ROADMAP marks done. Nothing named `Limiter` exists
  anywhere under `crates/crcbl-audio/src`. Mix snapshots and ducking are
  scheduled at P10 above that stage, and [32-voip.md](32-voip.md) schedules
  "mixer voice bus/ducking" with team voice against it too.

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

- **A backlog entry once framed the missing listener as an open design question,
  and it was not one.** The answer was already written in this document's
  architecture section, so what was owed was unbuilt work rather than a
  decision. That is the failure mode to watch for in the bullets around it.
