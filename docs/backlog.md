# Backlog

What was raised and not finished. A changelog says what shipped; this says what
did not, and why. Delete an entry when it ships — `git log` is the history.

## Owed, with a phase attached

The S1B findings in `docs/plan/ROADMAP.md` are the substantive list — six places
two unrelated games were pushed into the same workaround, each with the phase
that owes the fix. **Finding 1 is closed** (P4B bought `SpriteRenderer` and both
samples now draw through it); five stand. They are not repeated here; this file
carries what has no phase yet.

- **`Sprite` is a public-field struct, so every new field is a breaking
  change.** Adding `rotation` broke five literals in `apps/*/src/art.rs` and
  four inside `crcbl-render` itself, none of which had anything to do with
  turning. The next field — a pivot, a z, a flip — breaks them all again, and
  the sample count is going up. The fix is a constructor plus `with_*` methods
  (or `#[non_exhaustive]` and `..Default::default()`), decided before the field
  after next rather than after it. Not done in the rotation slice because it
  would have put an API refactor of every caller inside a rendering change.

- **The four samples still spell the audio entry point three different ways**,
  and the mixer-adoption slice deliberately did not unify them.
  `play_panned(id, emitter_x)` in breakout, `play_at(id, listener_x, x, y)` in
  flappy, `play_at(id, x, y)` in asteroids and `play_at(id, listener, at)` in
  horde. Each is right for its game's listener convention — fixed at the camera,
  fixed with one moving axis, fixed at the origin, riding the player — and the
  engine has no listener at all: `spatial::compute_cue` takes the listener
  position on every call and nothing in `crcbl-audio` remembers one. That is the
  real missing piece, and it is a design question rather than a refactor: a
  `Listener` on the mixer, set once a frame, would collapse all four signatures
  to `play_at(id, world_position)`. Not attempted here because it changes what
  the spatial grammar's API means, and the adoption slice was already touching
  four samples.

- **`VoiceMix::from(&SpatialCue)` drops `itd_samples` on the floor.** A `Voice`
  has no per-channel delay line, so the interaural time difference the cue
  grammar computes — rule 1's other half — reaches nothing. What survives is the
  gain difference, which is the direction without the timing. Documented on the
  `From` impl in `crates/crcbl-audio/src/mixer.rs` so it is not a silent loss.
  Fixing it means a fractional delay per channel inside `Voice::mix_block`, at a
  cost the audio thread has not been measured for.

- **`Mixer::stop` cuts a voice dead rather than fading it.** It removes the
  voice from the list immediately — which is what makes `voice_count` and
  `is_playing` answer without an audio thread to reap, and what horde's cap
  needs — but a loud voice stopped mid-cycle is a click. Asteroids' engine is
  the only caller and it is quiet enough not to matter. A short release ramp
  (mark stopped, fade over one block, then drop) is the fix and it would have to
  keep the immediate accounting: `stop` must still make room in the cap on the
  spot.

- **Nothing has listened to the migrated cues on a real device.** Every sample's
  audio was rewritten onto `crcbl_audio::mixer` and the checks are all
  structural: buffer shapes, pan ordering, voice counts, loop seams. Two
  audible-only risks are unverified. Asteroids' engine changed from a pulsed
  one-shot to a continuous loop at `ENGINE_GAIN` = 0.25 against the one-shots'
  0.5, and that ratio was chosen by reasoning rather than by hearing it. The
  loop seam is asserted to be a bare tone with no envelope, which is the right
  property, but nobody has heard whether ten joins a second is inaudible in
  practice. Both want a person with headphones.

- **The browser entry point was to be written once before S2, then before S3.
  THE DEADLINE HAS NOW BEEN MISSED TWICE.** Finding 2 in that list said so in as
  many words — "owed before S2, which will otherwise write it a third time" — S2
  wrote it a third time and re-owed it before S3, and S3 wrote a fourth.
  `apps/breakout/src/web.rs`, `apps/flappy/src/web.rs`,
  `apps/asteroids/src/web.rs` and `apps/horde/src/web.rs` are now the same file
  with four different symbol prefixes.

  **What it costs now, which is the part that changed.** The fix used to be
  "write it once, adopt it in two places". It is now one new shared
  implementation plus **four** call sites to migrate, four sets of `STATUS_*`
  constants to delete, four prefixes to thread through the macro, and four
  browser gates (`CRCBL_WEB_E2E_DEMO=…`) to re-run before it can be believed.
  Every sample after this adds one more of each. The four copies have still
  barely drifted — horde's was produced from asteroids' by substituting the
  sample name, and the executable difference is one `log::info!` line reporting
  a different summary — which is the one piece of good news. The `web.rs` four
  are now the _worst_ remaining copy of this shape:
  `apps/*/src/{best,high_score}.rs` still diverge in their public API, their
  type names, their file names and, as of horde, in **what they store** (three
  keep a score; horde keeps a time in whole seconds), and `apps/*/src/audio.rs`
  has been migrated onto `crcbl_audio::mixer` — what is left in each is the
  waveforms, the cue ids, the listener convention and horde's voice cap, all of
  which are genuinely per-game.

  What it would take: a crate (or a `crcbl` module) owning the `Stage` state
  machine, the log queue and the `prepare`/`boot`/`frame`/`status`/`shutdown`
  protocol, with the sample supplying only its prefix and its two loop types.
  The prefix has to stay per-sample — two demos can be open in one browser and
  the symbols must not collide — and `concat_idents!` is not stable, so the
  shape is a `macro_rules!` taking the ten symbol names as arguments over a
  generic core. **The JS half of this is done** — `web/engine/demo.js` is one
  boot sequence for every demo and `web/demos/<name>/main.js` is 33 lines — and
  it settles the shape question in the affirmative: the sample-specific part
  turned out to be ten literal symbol names and two strings, and nothing else.

  Why S2 and S3 both declined: it is an engine-API change to `crates/` plus
  edits to samples the slice was not otherwise touching, landing in the same
  commit as that game's audio, save file, demo page and — for S3 — its scale
  measurement. The JS half was done as its own piece of work for that reason and
  the Rust half should be too. **Do not attach it to the next sample slice.** It
  has now been attached to two of them and slipped both times; it wants a slice
  of its own, with the four browser gates as its exit criterion.

## The goal: a sample depends on `crcbl` and `std`, and on nothing else

Stated as a target for the samples on 2026-08-03. `apps/sandbox` already meets
it bar `log`; the four games do not, and the gap has two halves that want
separate work.

### Half one: twelve dependencies that are re-exports, not engine gaps

Each of `apps/{breakout,flappy,asteroids,horde}/Cargo.toml` names the umbrella
plus **eleven** more crates, and a build-dependency. None of it needs an engine
feature — it needs `crates/crcbl/src/lib.rs` to re-export what it already
depends on, or what nothing stops it depending on.

- **The nine engine crates with no re-export**: `crcbl-ecs`, `crcbl-net`,
  `crcbl-phys`, `crcbl-input`, `crcbl-server`, `crcbl-client`, `crcbl-audio`,
  `crcbl-store`, `crcbl-sprite`. **Cycle-free**: none of the nine depends on
  `crcbl`, checked by reading all nine manifests, so the re-export is nine
  `pub use` lines and nine manifest entries. `crcbl-scene` exists and no sample
  uses it.
- **`crcbl-core` is already `crcbl::core`** and the samples still write
  `crcbl_core::` — import churn, no engine change.
- **`glam` is already `crcbl::math`.** Twenty-one sample files import `glam::`
  directly. It is the same crate, so the types are identical and the change is
  mechanical.
- **`log`.** Verified rather than assumed: a scratch crate that does
  `pub use log;` and a consumer calling `logtest::log::info!` compiles, so
  `crcbl::log::info!` will work at all 143 sample call sites without a wrapper
  macro.
- **The `crcbl-sprite`/`bake` build-dependency is the one with a real
  trade-off.** Naming `crcbl` there instead means the build script links the
  whole engine — `crcbl-vk`, `crcbl-wgpu`, the renderer — to encode a PNG. The
  fix is to make the umbrella's heavy re-exports optional so a build script can
  take `default-features = false, features = ["bake"]`; the workspace is on
  `resolver = "3"`, which resolves build-dependency features separately from
  normal ones, so the game half keeps the full default set. **This is a
  decision, not a refactor**: a feature matrix on the umbrella is a public
  surface, and the alternative — leaving `crcbl-sprite` as the one permitted
  exception — is defensible.

### Half two: what is genuinely hand-rolled

Measured with comments and blank lines stripped, so these are shared _code_
lines rather than shared prose. Ranked by what the extraction buys.

1. **`web.rs`** — four copies, `breakout` vs `flappy` is 333 shared code lines
   against 32 that differ. Already the file's own section above; this is the
   number that section lacked.
2. **`app.rs`** — the loop. `breakout` vs `flappy` is ~1225 shared of ~1600.
3. **`args.rs` and `main.rs`** — `flappy` vs `asteroids` args is 200 shared
   against 8 differing; `main.rs` is 33 of 37. Four copies each.
4. **`best.rs` / `high_score.rs`** — 152 shared against 18. Four copies.
5. **The waveform synthesis in `audio.rs` is the newest find and the cleanest
   case.** `fn sine` is **byte-identical** across flappy, asteroids and horde
   (11 lines, confirmed with `cmp`, not with a diff tool), and so is its `fade`
   helper (14 lines). Breakout has the same two functions under the older names
   `gen_sine` and `fade_env`. `crcbl-audio` has a mixer, a sound bank and a
   spatial grammar but **no oscillator and no envelope**, so every sample that
   wants a beep writes one. An engine `synth` module — a sine, a noise source
   and a linear fade — closes it; asteroids' `looped_sine` and horde's decaying
   `noise` say what the shape has to cover.
6. **The loopback session.** All four games build `InMemoryTransport::pair()` →
   `Server::try_new_with_compatibility` → `Client::new_with_compatibility` with
   a per-game `ProtocolCompatibility` const. Single-player-is-a-loopback-server
   is the engine's architectural decision, not each game's, and nothing in
   `crcbl` expresses it.
7. **`crcbl_ui::hud` exists and no sample uses it.** `Hud`, `HudPanel` and
   `Anchor` are in `crates/crcbl-ui/src/hud.rs`; every sample instead has a
   private `HudStrings` in its `app.rs`. This is adoption, not extraction — the
   engine feature was already bought.

**`gpu.rs` and `menu.rs` are NOT on this list, and that is the good news.** Both
were extracted already — `crcbl::engine::GpuContext` and
`crcbl::ui::menu::MenuSet` — and what is left measures as genuinely per-game:
`gpu.rs` is 232 shared against 152 differing, `menu.rs` 266 against 131, where
the differences are cameras, pass order, menu kinds and button labels. They are
the shape the other seven should end up in.

## The rustdoc gate never documents the wasm target, so every `web.rs` is unchecked

CI runs
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` on
the host target only, and `apps/*/src/web.rs` is behind
`#[cfg(target_arch = "wasm32")]`. So the one module in each sample that is pure
documentation — a symbol table, a call-ordering diagram and the ABI contract the
JS shim is written against — is the one module the docs gate has never read.

Measured, not assumed.
`cargo doc -p <sample> --no-deps --target wasm32-unknown-unknown` with the same
`RUSTDOCFLAGS` reports **4 errors for breakout and 5 for flappy** today:
`unresolved link to crcbl_shell` (the crate is reachable only as
`crcbl::shell`), public docs linking to the private `WebLogger`, `Stage` and
`crate::best`, and a redundant explicit link target. `apps/asteroids/src/web.rs`
had the same set and was fixed as it was written, so it passes;
`apps/horde/src/web.rs` inherited the fixed version and was checked the same way
(`cargo doc -p horde --no-deps --target wasm32-unknown-unknown` with
`RUSTDOCFLAGS="-D warnings"` is clean). The other two are untouched because
neither slice's write scope included them.

The fix is one line in `.github/workflows/ci.yml` — a second `cargo doc` step
with `--target wasm32-unknown-unknown` over the four sample crates — plus the
nine link fixes it would then demand. Not done here because adding a required CI
job that fails on two crates this slice may not edit would land the tree red.

## The Windows `crcbl-cli` fixtures are fixed but nothing has run them on Windows

The three tests that had `build + test (windows-latest)` red since `216ea85` —
`crpix_refuses_frames_of_different_sizes_and_names_the_file` and
`crpix_fails_cleanly_on_a_missing_file_and_on_a_file_that_is_not_a_png` in
`crates/crcbl-cli/tests/cli.rs`, and `a_stem_the_format_cannot_spell_is_refused`
in `crates/crcbl-cli/src/crpix_cmd.rs` — were fixtures asserting a Unix
assumption, not defects in the CLI, and the fixtures now assert the property
they meant to on both platforms. No case was dropped and no `cfg` was added.

What is **not** verified: this machine cannot compile or run anything for
Windows, so the claim that `art/a:b.png` has the stem `a:b` there rests on
reading `parse_prefix` in `library/std/src/sys/path/windows_prefix.rs` — a drive
prefix is matched as `[drive, b':', ..]` against the whole path's bytes, once,
never against a later component. The escaping half is exercised on Linux, by a
fixture whose file _name_ holds a backslash, so that half no longer depends on a
Windows runner. Only a green `build + test (windows-latest)` closes the rest.

Nothing else about `crpix` on Windows has been reviewed. These three are what CI
reported, not the result of an audit, so a second Windows-only failure elsewhere
in the command would be a new finding rather than a regression.

## `run-vk-e2e.sh` pins no ICD by default, so nobody was running CI's gate

The script's header says it "is what a developer runs to see what CI sees". That
was not true unless the developer happened to set `CRCBL_VK_ICD`: the pin block
is wrapped in `if [ -n "${CRCBL_VK_ICD:-}" ]`, and with the variable unset the
script exports nothing and the Vulkan loader picks whatever is installed. On a
workstation that is the discrete GPU. Measured here: a bare
`crates/crcbl-vk/tests/run-vk-e2e.sh` reports
`adapter "AMD Radeon RX 7900 XTX (RADV NAVI31)"`, while CI's job sets
`CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` and gets llvmpipe.
The suite printed the adapter it got all along, but an adapter line is not
something anyone reads looking for an absence.

The script now prints a warning naming the gap and the command that closes it,
and still runs — testing against real hardware deliberately is worth doing, and
this is how. **It is still not a hard failure**, which is a judgement worth
revisiting: the alternative is defaulting `CRCBL_VK_ICD` to lavapipe so the bare
invocation is CI's invocation, and requiring an explicit opt-out to use the real
GPU. That is probably the better default and was not taken here because it
changes what an existing command does.

## This machine's validation layer cannot see the two-submission hazard

`crcbl-vk`'s offscreen-ring write-after-read (fixed — see `CHANGELOG.md`) is
reported by CI's layer and by nothing here, and the difference is the **layer**,
not the ICD. Measured under CI's own ICD, so the ICD is ruled out:
`CRCBL_VK_ICD=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json` (which the script
resolves to Arch's `lvp_icd.json`), adapter
`llvmpipe (Mesa 26.1.6-arch1.1 (LLVM 22.1.8))`, layer
`VK_LAYER_KHRONOS_validation spec 1.4.357` — and
`synchronisation_validation_catches_a_missing_barrier` still reports
`record-time=yes one-submission=yes cross-submission=no`.

`syncval_submit_time_validation` **defaults to true** in that layer build (read
out of `/usr/share/vulkan/explicit_layer.d/VkLayer_khronos_validation.json`),
and setting it explicitly through `VK_LAYER_SETTINGS_PATH` and through the
`VK_KHRONOS_VALIDATION_*` environment form both left the measured reach
unchanged. So the switch is not the explanation and there is no known way to
widen it here. What that costs: the **two-submission** instance of this bug can
only be observed in CI.

What stands in for it locally is
`reusing_an_offscreen_ring_image_is_ordered_against_the_frame_that_had_it` in
`crates/crcbl-vk/tests/vk_e2e.rs`, which provokes the same missing dependency at
record-time distance — a one-image ring, both trips recorded into one command
buffer — where every layer build sees it. It was falsified by disabling the
widening in `VkCommandEncoder::pipeline_barrier`: red, with the layer naming
`vkCmdPipelineBarrier2 performs image layout transition on the VkImage ... which was previously read by vkCmdCopyImageToBuffer`.

**A CPU wait was tried first and does not work**, which is worth keeping because
it is a plausible idea. `acquire_next_frame` blocked on the retire timeline
before handing a reused image back. Instrumentation confirmed it ran (`reuse=3`
and `reuse=4` on the third and fourth frames of the failing test), and CI
reported the identical hazard anyway: a host-side wait establishes real ordering
but is not a queue dependency, and syncval reasons about submitted commands. It
also costs exactly the frame overlap the ring exists to provide. It was removed
rather than kept alongside the barrier.

Still **not** checked: whether `crcbl-wgpu`'s offscreen path has the same gap.
Its acquire also reports `acquire_semaphore: None`, but wgpu owns the
synchronisation behind it and `wgpu e2e (lavapipe, Xvfb)` is green — no evidence
either way, only an untested assumption.

## The demo site's social preview is blank

No `og:image`, so every link posted to Slack, Discord or a Mastodon timeline
renders as a bare title and description. `web/templates/layout.html` is where
the tag would go and the site already carries `og:title`, `og:description`,
`og:type`, `og:url` and `og:site_name`, so the tag itself is one line — the
missing half is the image. It cannot be the `favicon.svg`: the platforms that
matter want a raster of about 1200×630, and several ignore SVG entirely.

Deliberately not invented here, because the choice is a design one: a rendered
frame of a demo, a wordmark on the site's own background, or a per-demo card.
Whatever it is has to be baked from a committed source the way
`crates/crcbl-render/assets/menu.crpix` is, not a PNG dropped into `web/` with
no way to regenerate it.

## `/favicon.ico` is still a 404, deliberately

`web/favicon.svg` is declared by the layout, which is what stops the browsers in
the requirements list (Chrome/Edge 113+, Safari 18+, Firefox) asking for
`/favicon.ico` at all — verified: `curl` against the live site returned 404 for
that path before the change, and the built pages now carry
`<link rel="icon" href="/favicon.svg">`. A browser that ignores the declaration
still gets a 404 and no icon.

Not fixed because an `.ico` is a binary blob and this repo bakes its art from
committed text. `web/build.sh` has no image toolchain and adding one for a 16×16
icon is a worse trade than the miss. `web/tools/browser-e2e.mjs` still filters
`favicon.ico` out of its 404 assertion for the same reason.

## Always run the browser gate with `--build`

A run without `--build` uses whatever is in `target/site`, and that directory
outlives the commit it was built from — a stale site is how a green run gets
reported for code that is red. This has cost real time once already: both demos
were signed off at "25/25" against a site built before the focus, pause and menu
work landed, and the gate had in fact been red the whole time. Pass `--build`,
or delete `target/site` first.

## Should the click that refocuses a canvas reach the game at all?

**Behaviour that surprised us, deliberately left alone.** A canvas has no title
bar, so `web/engine/shell.js` gives it the keyboard from its own `pointerdown`
handler — which makes the click that "clicks back into the window" also a press
at a real position inside the game. With the pause menu on screen and `RESUME`
under the cursor, clicking back in resumes. That is each half behaving correctly
and the combination being surprising; it is what put the browser gate at 23/25
for a slice, because section E clicked the canvas's _centre_ to restore focus
and the menu is centred there.

The alternative is click-to-focus **activation blocking**: the first press after
a focus gain restores focus and is swallowed rather than delivered, which is
what several desktop toolkits do. Not done, and not obviously right — swallowing
a click is its own surprise, and for a paused game the current behaviour is
arguably the friendlier one (the player clicked on `RESUME`; they got a resume).
It needs a decision rather than a patch, and it would have to be decided for
native and web together, since `Loop` cannot tell the two apart.

What holds the line meanwhile:
`a_focusing_click_off_every_button_leaves_the_game_paused` in all three samples'
`app.rs` asserts the corner is over no button and the centre is over `RESUME`,
so a menu that grew until it reached the corner fails a fast Rust test rather
than the slow browser one. Three copies of it, because the menu geometry is
per-sample even though `FOCUS_CLICK_INSET` is not — the same shape as everything
else in `apps/*/src/app.rs`.

## The sprite system, and what is left of the retrofit

The pipeline is joined up end to end for both games: each of `apps/flappy` and
`apps/breakout` authors `.crpix` text under `assets/`, a `build.rs` bakes it,
and an `art::Scene` draws it through `SpriteRenderer` on a layer stack. What is
left:

- **`NineSliceSource` has no texels → units scale, and there are now two callers
  working around it.** `expand` takes its insets as target units directly, so a
  6-texel cap is 6 units tall whatever the caller's world is. Flappy scaled its
  whole sprite plane by `art::TEXELS_PER_UNIT` = 20 so its pipe's cap would
  survive a 12-unit playable band; breakout hit the identical wall on
  `assets/field.crpix`, whose 10-texel walls would otherwise be ten world units
  thick inside a court 28 across, and reached the same convention at a scale of
  10 — chosen independently, from the ball rather than from the pipe, which is
  the only part that did not copy.

  **This was the "if a second game hits this" condition, and it has been hit.**
  It was not fixed in that slice because the slice was scoped to `apps/*` and
  the change is in `crcbl-render`. The fix: a scale on `NineSliceSource` — a
  `texels_per_unit: f32` field set at `from_sheet` time, or an
  `expand_scaled(target, scale)` beside `expand` — so `minimum_size` and the
  fixed bands come back in the caller's units and a game whose world is not one
  unit per texel does not have to scale its camera to compensate. Both
  `art::TEXELS_PER_UNIT` constants and both `gpu::projection` multiplications
  come out when it lands; the sprite rectangles stay as they are, because those
  were never the problem. Nothing else in the workspace calls `expand` —
  `crcbl-render`'s own `button_skin` does, and would take the same scale of 1.

  **A third caller has now appeared, inside `crcbl-render` itself.**
  `crcbl_render::menu_camera` exists only because of this: the menu is laid out
  in device pixels, and drawing it through a camera of one unit per pixel would
  make the window frame's four-texel corner four _pixels_ at every scale, so a
  menu drawn three times as large would keep a hairline border. The camera
  divides by the style's scale instead, exactly as the two samples scale theirs.
  It comes out with the other two when `expand` learns a scale.

- **The bake half of `build.rs` is written five times, and it got worse again.**
  `apps/flappy/build.rs`, `apps/breakout/build.rs`,
  `crates/crcbl-render/build.rs`, `apps/asteroids/build.rs` and now
  `apps/horde/build.rs` differ in their `ASSETS` array and in nothing else: the
  same parse → bake → write → generate-a-table loop, the same `ART_TICK_HZ`, the
  same `cargo::error` reporting. `docs/plan/ROADMAP.md` says this was owed
  **before the third sample**; the third and the fourth both shipped with a copy
  instead, because closing it is a change to `crcbl-sprite` and to four other
  build scripts and the slice that would have paid for it was, both times, the
  art slice. The fix is unchanged: a real entry point in `crcbl-sprite` —
  something like `bake::bake_dir(manifest_dir, out_dir, &stems, tick_hz)`
  returning the table text — because a build script can depend on a workspace
  library and that is the only shape that removes the copy rather than moving
  it. It is now the cheapest of the five duplications to close and the one with
  the most copies.

- **The tick rate the art is baked at is written twice per game** —
  `ART_TICK_HZ` in `apps/*/build.rs` and again in `apps/*/src/art.rs`. A build
  script cannot `use` the crate it builds, and the sidecar's durations are
  milliseconds, so the two conversions have to agree. Guarded rather than
  solved: each game's `the_art_bakes_to_the_sheets_it_declares` asserts an
  authored hold in ticks survives the round trip. **Breakout's and asteroids'
  guards are weaker than flappy's**, because neither draws anything animated:
  both can only assert the default hold of 1 tick, which survives a fairly wide
  range of wrong rates. Asteroids' ship and rocks _turn_, which is a rotation
  applied to a still frame and not a clip, so it does not help. Either gets real
  the moment that game has a clip. Folding it into the `bake_dir` entry point
  above would close it outright.

- **Breakout's paddle is a plain frame, not a nine-slice.**
  `game::PADDLE_HALF_WIDTH` is a `const` and nothing shadows it, so the paddle
  is 10 world units across on every tick of every run and a stretch would have
  had no caller. If a widening power-up is ever added, `assets/paddle.crpix`
  wants `nine: 12 12 0 0` and `art::paddle_rect` already produces the target
  rectangle `expand` would take.

- **`Sprite::rotation` has no pivot offset, and that was a decision.** The angle
  turns the quad about the centre of its own `rect` and there is no way to name
  another point. Considered and declined: the sheet lane has exactly one
  component left, an offset needs two, and the case it would serve is
  expressible without it — a rectangle rotated about an outside point is the
  same rectangle rotated about its own centre and then translated, so a caller
  wanting an orbit computes the translated `rect` and gets it exactly. Reopen it
  if something wants a pivot that is _animated_ independently of position, which
  is the one shape the translation trick makes awkward; it would need a fifth
  `float4` on the instance, taking it from 64 bytes to 80.

- **A nine-slice cannot be rotated, and neither can a menu or a button skin.**
  `NineQuads::sprites` hard-codes `rotation: 0.0` with a comment saying why: the
  nine quads are stretched against each other, so turning each about its own
  centre opens a gap at every band boundary, and turning the frame as a whole
  needs one pivot shared by all nine — which is a different feature from
  `Sprite::rotation`, and would be `expand`'s job rather than the instance's.
  Nothing has asked for it. If something does, the shape is a rotation on
  `NineSliceSource::expand`'s target that it applies to all nine quads about the
  target's centre, which needs the per-sprite pivot above or a rect-plus-angle
  that is not the rect's own centre.

- **The angle a sample interpolates is the sample's own, and the third one will
  copy it.** Asteroids answered the open question — `game::lerp_angle`, a
  shortest-arc scalar lerp between the previous tick's angle and this one,
  driven by `FrameClock::alpha` — but it answered it _in the sample_. Nothing in
  `crcbl-render` or `crcbl-client` offers it: `Client::interpolate` lerps
  `Transform`s, whose `DQuat` no sprite path reads, and `crcbl-phys` still has
  no angular velocity, so game code owns every angle. The next sample that turns
  something writes the same twenty lines. Worth promoting only when there is a
  second caller — the shape would be a `lerp_angle` in `crcbl-core`'s math, or a
  `Sprite` that could take the pair and the alpha, and the second is a much
  bigger claim than it looks.

## The debug overlay, and what is left of it

The modular panel is built and all three samples switch it on with F3 (or
`--debug-overlay`): `crcbl_ui::debug` owns `DebugPanel`, `DebugSection`,
`DebugModule`, `FrameStats` and `DebugOverlay`; `crcbl-render` contributes the
`gpu` section by implementing `DebugModule` for `FrameTimings`. What is left:

- **There is no network module, and no sample could show one yet.** The panel is
  ready for it — a module is a `DebugModule` impl and one `add` call — but
  nothing was written, for two reasons. The first is that `23-netcode.md`'s
  netgraph list (RTT, jitter, loss, send/recv bandwidth, snapshot size, resend
  counts, tick-lead) is **not measurable today**: `InMemoryTransport` has no
  timing, no loss and no byte accounting, and `Client` exposes only
  `is_connected`, `session_id`, `last_applied_tick`, `baseline_entity_count`,
  `baseline_system_count`, `processing_error_count`, `auth_failure_count` and
  `rate_limited_message_count`/`rate_limited_byte_count`. Those are real numbers
  and a module could show them, but they are a connection-health readout, not a
  netgraph, and shipping them under that name would make the P10 work look done.
  The second is placement: the module belongs in the crate that owns the numbers
  (`crcbl-client`, following `crcbl-render`'s example), which means a
  `crcbl-client → crcbl-ui` dependency. That is not obviously wrong — `crcbl-ui`
  depends on nothing but `glam` and `bytemuck`, so there is no cycle — but it is
  the first time a simulation crate would depend on the UI, and it is a call
  worth making deliberately rather than in passing. **What it would take**: the
  transport growing byte and timing counters, then a `DebugModule` impl beside
  them, then one `add` line in each sample that has a connection.
- **The wiring is four lines, repeated three times.** Every sample's
  `draw_debug_overlay` is the same: `begin_frame`, offer the GPU timings, render
  into the draw list at the swapchain's extent. It is short enough not to hurt
  at three copies and it is exactly the shape `web.rs` took before it became a
  finding. The right home is `crcbl::engine`, next to `GpuContext` — the place
  the shell and the HAL already meet — but that crate was out of scope for the
  slice that built this. Fold it in when the fourth sample arrives, at the
  latest.
- **The panel's own cost is measured at 960 × 720 and still not at 1080p.**
  Horde's scale runs took it: with ten thousand enemies on the field and the
  panel showing three sections, switching it on moves the `ui-composite` GPU
  pass from 0.004 ms to 0.005 ms and leaves the CPU frame time inside its own
  noise (0.107/0.109 ms off against 0.100/0.101 ms on, two runs each — the
  "with" runs came out _lower_, which is the noise floor rather than a saving).
  `07-ui-debug.md`'s criterion is **"<0.5 ms GPU at 1080p"** and that extent has
  not been run, so the criterion is not closed; it is two orders of magnitude
  the right side of it at three quarters the pixels. Conditions: release,
  `--backend vk` on radv (RX 7900 XTX), headless offscreen ring,
  `--wall-clock --tick-hz 1 --frames 900 --prefill 10000`, `PassTimers::latest`
  for the GPU number and the panel's own `FrameStats` mean for the CPU one.
- **The overlay starts hidden in a release wasm build.** The default is
  `cfg!(debug_assertions)`, which is sample rule 4's "on by default in dev
  builds" taken literally; the demos on `crcbl.kryptic.sh` are release builds,
  so a visitor has to press F3. Whether the published demos should default it on
  is a product decision nobody has made. `web.rs` builds `Options::default()`,
  so turning it on there is one field.

## Coverage gaps

- **No sample test pins the _values_ its RNG produces — only that two runs
  agree.** Found by falsifying `crcbl_core::rand::hash_u64`: dropping its final
  xor-shift left asteroids, flappy, horde and `crcbl-phys` **entirely green**,
  and only `crcbl-core`'s own tests went red. The determinism suites compare one
  run against another, and both runs move together when the hash changes, so a
  rewrite would silently redraw every course, board and horde in the project
  with nothing failing. `it_matches_the_reference_vectors` now pins the shared
  implementation against the published splitmix64 outputs, which covers all four
  games because they all call it — but each game's own mapping from a hash to a
  gap centre or a spawn ring is still unpinned, so a change to `gap_centre`'s
  arithmetic has the same free hand the hash used to. What would close it: one
  test per game asserting a couple of literal positions for a fixed seed.

- **The mixer adoption was not verified by ear, and two of its choices are
  audible-only.** See the entry under _Owed_ above. Structurally everything is
  pinned; nothing has been listened to.

- **`Mixer` is exercised single-threaded in every sample test.** The engine has
  `mixer_is_sync_and_fill_is_serialised`, which drives four threads through
  `fill` while voices loop, but nothing tests `play`/`stop`/`set_mix` racing
  against a live `fill` — which is exactly what happens in a real game, where
  the game thread calls all three while the audio callback runs. The `Mutex`
  makes it safe by construction and no test says so.

- **The `wasm32` audio path is not built by the local verification loop.**
  `AudioStream::open` on `wasm32` goes through `web::install`, and the blanket
  `impl AudioSource for Arc<T>` is what makes an `Arc<Mixer>` acceptable there
  too. The browser gate (`web/run-browser-e2e.sh --build`) covers the four demos
  end to end, which is the only place that path runs.

- **Nothing checks the demo site's HTML in CI.** The 2026-08-02 audit ran
  `npx html-validate` (recommended + document + a11y presets) and a stdlib
  Python parser over the three built pages, plus `curl` over every external link
  and a headless-Chromium screenshot at 1280 and at 390 wide. All of that was a
  human running commands; `web/build.sh` runs `check-exports.mjs` and
  `smoke.mjs` and nothing that reads the HTML it just wrote. What it would take,
  in the no-npm spirit of `web/`: fold the link-and-asset resolution check into
  `build-pages.py`, which already knows every page it wrote and every file the
  site will contain. The tag-balance half is what `html-validate` does better,
  and that one needs a dependency.
- **`html-validate` reports `require-sri` on every `<link>` and `<script>`, and
  it is being ignored.** Subresource Integrity guards a resource served by
  someone else; the stylesheet and the demo shims are same-origin files this
  repo builds in the same step as the page that names them, and a hash pinned in
  the layout would have to be regenerated on every edit to `style.css`. Recorded
  so the next person to run the validator does not re-litigate it.
- **No visual regression baseline for the site.** The browser gate captures the
  _canvas_ — deliberately, since the page's chrome is not what it tests — so a
  stylesheet or template change that breaks the layout around the canvas would
  pass all 26 checks. The screenshots taken during the 2026-08-02 audit were
  looked at by a human and thrown away.
- **The menu golden cannot see an inset larger than the one authored.**
  `menu_frame_two_sizes` compares the two panels' corner blocks pixel for pixel,
  which catches a corner that _scaled_ with the target — measured: making the
  panel's insets a function of the target width failed it. It cannot catch an
  inset that grew uniformly, because `menu.crpix`'s panel is uniform fill past
  texel 3 on both axes, so a `nine` of 6 draws exactly the same picture as a
  `nine` of 4. That number is pinned instead by
  `crcbl_render::menu::the_shipped_art_has_the_insets_the_layout_assumes` and by
  the layout tests, both of which go red on it.
- **The golden's reference is weak on small-area art changes.** The image is
  416×576, so recolouring a one-texel band moves under 1% of the pixels and
  compares inside `Tolerance::RASTERISER` — measured: swapping the panel's
  shadow colour for its own channels reversed passed the reference. The pixel
  assertions carry that weight instead (`assert_menu_pixels` samples the
  highlight and the shadow on all four edges, and that _did_ catch it). Worth
  knowing before adding a claim to this golden that only the reference would
  hold.
- **Nothing has looked at a menu over a real game.** The golden renders the menu
  over a flat clear colour on an offscreen ring; no test composites one over
  breakout's brick grid or flappy's course, and no human has confirmed the scrim
  reads well over either. The browser gate reaches a paused demo but only counts
  HUD lines. The browser gate's canvas capture is the closest thing there is,
  and it happens to fire while the pause menu is up — a human has now looked at
  it for both demos and it reads fine, but nothing asserts it.
- **No test and no tool captures a native sample's pixels.** `breakout` and
  `flappy` take `--headless --frames N` and print a summary; neither has a
  screenshot path, so "the Vulkan build of the game draws the right picture" is
  reachable only by running it on a desktop. The multi-sheet bug lived in the
  shipped samples for that reason: the evidence that it is gone is
  `every_batch_draws_its_own_instances_rather_than_the_first_batchs` exercising
  the same `SpriteRenderer` on radv, plus both samples running 120 headless
  frames with `CRCBL_VK_VALIDATION` and sync validation clean — **not** a
  picture of either game. What it would take: a `--capture <path>` on the sample
  front ends, reading the swapchain image back the way `vk_e2e.rs`'s
  `render_sprites` does.
- **Four samples' `switching_menus_drops_the_press` asserts nothing.**
  `apps/breakout/src/menu.rs`, `apps/flappy/src/menu.rs`,
  `apps/asteroids/src/menu.rs` and `apps/horde/src/menu.rs` each press the
  **first** button of one menu and then release over the first button of
  another. `UiState::interact` fires on release only when the capture names the
  same `WidgetId` the cursor is over, and no two of those samples' menus share
  an id in slot 0 — breakout's pause menu opens with `Resume`, its start menu
  with `Launch` — so the release could not have fired whatever the container
  did, and the test passes with the capture never cleared at all. Its other
  assertion is vacuous the same way: the menu being switched _to_ was never
  pressed, so "the new menu inherited a press" cannot be observed on it.

  Verified by falsification, not by reading: deleting `self.ui.clear()` from
  `MenuSet::show` leaves all four green. This predates the extraction — the
  tests are the originals, unchanged in shape — and `crcbl-ui`'s own
  `switching_menus_drops_the_press` now covers the behaviour with teeth (it goes
  red, because it presses the `FULLSCREEN` button both menus carry under one
  id). **Not fixed here** because the behaviour is entirely the engine's now and
  a per-sample copy of it is redundant; what would make the four bite is
  pressing slot 1 rather than slot 0, except in horde, whose pause and level-up
  menus share no id at all and which would have to switch pause → start instead.
  `apps/sandbox/src/menu.rs`'s `hiding_the_menu_drops_the_press` does bite, and
  horde's `a_new_offer_drops_the_press` does too — both confirmed red.

- **The loop's pause / fullscreen / focus / pointer-capture block is written
  four times.** `apps/breakout/src/app.rs`, `apps/flappy/src/app.rs`,
  `apps/asteroids/src/app.rs` and `apps/horde/src/app.rs` carry the same
  `Loop::paused` field, the same `lose_focus` (drain the held keys, then pause),
  the same F11 `toggle_fullscreen` reading the mode back rather than remembering
  it, the same "drain the accumulator while paused" tick loop, and the same
  `pointer_held` / `pointer_down` press-capture bookkeeping in the pump. The
  shape is a `SampleLoop` helper owning the flags and the pump's non-game
  branches, with the sample supplying its own key bindings and its own
  `MenuAction` handler.

  **Most of that slice has now landed**, in pieces, each with its own commit:
  `crcbl_ui::menu::MenuSet`, then `crcbl::engine::LoopError<G>`, `open_window`
  and `MAX_FRAME_STEP`, `PolledBoot`/`PolledGpu`, and `MenuPump` with the three
  menu keys. `web.rs` went the same way — `crcbl::web` took the status codes,
  the log queue and the whole `App` lifecycle.

  **What is left is the body of `Loop::frame` itself.** Measured on the current
  tree with the game names normalised away and comments and tests stripped:
  breakout and flappy are 511 and 508 code lines with **480 identical**, which
  past rustfmt's wrapping is still the same file.

  The remaining shared parts are the fixed-step accumulator with its
  drain-while-paused rule, the pointer press-capture bookkeeping, the post-pump
  batch resolution (debug overlay, focus loss, pause, fullscreen, mode request,
  destroyed, close, resize), the present/reconfigure counting, `finish`, and
  `run`. The genuinely per-game parts are `assemble`, `apply` (the `MenuAction`
  handler), `draw_hud`/`HudStrings`, `menu_kind`, and the game constants.

  **Why what was left needed a decision first.** Every extraction had a seam
  that was already a type: an error, a window, a boot state machine, a menu
  batch, a pointer, a mode request, a frame budget, a driver. What remained had
  none, and the longest shared runs said why. Measured on breakout against
  flappy, names normalised, comments and tests stripped — 431 and 427 code lines
  with **399 identical**, in runs of:

  | lines | what it is                                              |
  | ----- | ------------------------------------------------------- |
  | 150   | `apply`, `draw_menu`, `menu_kind`, `draw_debug_overlay` |
  | 100   | `assemble`'s struct literal and `frame`'s prologue      |
  | 53    | the `Loop` **struct definition** itself                 |
  | 29    | the `use` block                                         |
  | 25    | `frame`'s draw-and-present tail                         |

  A struct's field list cannot be extracted without owning the struct, so the
  rest goes only if the engine owns the loop and the game plugs into it.

  **Decided, and the engine half has landed.** `crcbl::engine::Loop` owns the
  frame, `HostedGame` is the seam, `GameGpu` is the frame's half of a game's GPU
  bundle, and `apps/bare` plus `crates/crcbl/tests/library_seam.rs` are the
  guard that a game can still decline all of it and write its own loop. The
  runner stays swappable — `drive` natively, `crcbl::web::App` in the browser,
  both over `GameLoop`.

  **Breakout is converted; four samples are not.** `apps/breakout` was the first
  consumer and cost `app.rs` 309 lines and `web.rs` 27, with its own 79 tests
  passing unmodified except where they reached a field now behind an accessor,
  and the browser gate green at 27/27 against a real WebGPU device.

  What breakout needed that the seam did not have, found by converting it: a
  `HostedGame::NAME` and `HostedGame::log_summary`, so `crcbl::web` could
  blanket-implement `WebLoop` — a sample cannot implement a foreign trait for
  the engine's foreign `Loop`, and without the blanket impl every sample would
  keep a five-forward `WebLoop` block. Also
  `Loop::{debug, ticks, held_keys, mode_honoured, clock_source}`, each because a
  breakout test read the field directly.

  **Flappy followed and cost the seam nothing** — no new trait item, no new
  accessor; its only unusual need, `gpu.advance_animation(ticks_this_frame)`,
  was already carried by `FrameInfo::ticks`. `app.rs` lost 288 lines, `web.rs`
  28, its 86 tests pass and its browser gate ran 27/27.

  asteroids and horde should be next and should be the same shape; horde's menu
  has two game actions (`Restart` and `Choose(n)`), so it is the first to
  exercise more than one `WidgetId` above `FIRST_GAME_ID`. `apps/sandbox` is the
  one to watch: it has no `MenuKind`, its tick body touches its GPU
  (`gpu.advance(dt)`, which is why `HostedGame::tick` takes `&mut Self::Gpu`),
  and it reads `alpha` after the tick loop (which is why `FrameInfo` carries
  it). None of that is verified against sandbox — it was read out of
  `apps/sandbox/src/app.rs` while designing the trait, not compiled.

- **asteroids and horde never report a refused fullscreen.** Found while
  extracting `ModeRequest`: breakout, flappy and sandbox all call
  `check_mode_request` once a frame, and those two have no such call and no
  equivalent. A player on a tiling window manager presses F11 in asteroids and
  gets no window change and no log line saying why. **Not fixed there**, because
  adding a call that starts emitting warnings is a behaviour change and the
  commit it was found in was an extraction. The fix is one line in each of the
  two `frame` bodies, now that `ModeRequest::check` exists.

  **The error half has landed** — `crcbl::engine::LoopError<G>` — and it cost
  far less than this entry predicted, so the prediction is worth correcting
  rather than deleting. It said a shared error "needs generics or boxing, and
  touches every signature in five binaries", because the `From` impls name each
  crate's own `gpu::GpuError`. There is no such type. The four games import
  `GpuError` straight from `crcbl::engine`, and `apps/sandbox/src/gpu.rs`
  `pub use`s the same one — so only `game::GameError` was ever per-crate, and
  one type parameter carried it. No signature changed — `BreakoutError` and its
  four siblings are aliases, so
  `fn frame(&mut self) -> Result<Flow, BreakoutError>` reads as it did and
  `Err(BreakoutError::Gpu(…))` still constructs. The one call site that changed
  per game is the game's own constructor, from `?` to
  `.map_err(BreakoutError::Game)`, because a blanket `From<G>` overlaps the
  three concrete ones. 244 lines out of the five `app.rs` files against 84 of
  shared type and 68 of tests in the engine.

  The lesson for the rest of this slice: the "five binaries with different
  types" framing was itself the overestimate. Check what a sample's local name
  actually resolves to before pricing an extraction around it.

  What the extraction has to carry, found while doing the menu half:
  - The pump's menu branches call `select_previous` / `select_next` / `press` /
    `activate` on a `MenuSet<K>` whose `K` is the sample's own enum, so an
    extracted pump is generic over `K` or takes buffered menu commands and lets
    the caller replay them. Buffering is behaviour-preserving — nothing else in
    the pump reads menu state, and `menu_showing` is sampled once _before_ the
    pump — but it is a real change of shape and wants saying out loud.
  - The pump captures `&mut self.game` and calls
    `game.key_event(code, pressed)`; a closure argument covers it, but
    `held_keys` is mutated in the same branch and belongs to the loop, not the
    game.
  - Sandbox is the fifth consumer of `run`, `BootStage` and `check_mode_request`
    and the only one with no `MenuKind`; it must be in the slice, not after it.

- **Flappy's swept-sphere collision is exercised, not demonstrated.**
  `game::fatal` sweeps the bird's path with `PhysicsSystem::sweep_sphere`
  because that is the correct query, but at this game's speeds a point test at
  the end of the tick catches every pipe the swept one does — measured, down to
  a tick rate of 3 Hz, where the bird still ends each tick inside the pipe it
  would have tunnelled through. Tunnelling needs a step wider than a pipe plus a
  bird (2.3 units, so under about 2.6 Hz), which is not a rate this game is
  coherent at. Closing it honestly means a faster consumer, not a contrived test
  here.
- **No golden image covers the play field's framing, in either sample.** The
  browser gate reads the canvas back and asserts it is neither blank nor still.
  Flappy's `gpu::the_playable_band_is_on_screen_at_every_aspect_ratio` now puts
  the world through the real view-projection at five aspect ratios, which is
  stronger than the hand-written mapping it replaced, and is still not a pixel
  check that would catch the framing drifting.
- **Only the sandbox asserts that its UI pass reaches the render graph.**
  `f3_toggles_the_debug_overlay_in_the_sandbox` reads the frame's graph dump and
  requires `ui-composite` to be present when the overlay is on and absent when
  it is off, which is what makes "the overlay was drawn" mean "the overlay was
  composited". Breakout and flappy have no equivalent — their tests stop at the
  draw list reaching `Gpu` — because neither `Gpu` keeps the dump. This predates
  the overlay: nothing ever asserted their HUD's pass either. Closing it is a
  `#[cfg(test)] last_dump: String` in each, the same three lines the sandbox now
  has.
- **The overlay has never been looked at.** Every test over it is draw-list
  strings and rectangles; no golden image, and no human has confirmed the panel
  is legible over a lit scene or a bright sprite background. The layout maths
  (value column past the longest label, panel inside the screen at two sizes) is
  asserted; the _appearance_ is not.
- **Nothing has looked at either sample's art come out of a GPU.** Every test
  over it is `Sheet` data, sprite rectangles and layer membership; the pictures
  in both retrofit reports were composited in software from the same sprite
  lists, so they say the scenes are assembled correctly and nothing about the
  shader, the sampler or the blend. `crcbl screenshot` cannot help — it renders
  the sandbox cube through `ForwardRenderer`, which **neither** sample uses now.
  Closing this means either a golden through the sprite pass with a sample's own
  sheets, or an offscreen path the samples can drive. It is the same gap for
  breakout as for flappy and is not worth two entries.
- **The bird sprite is not checked against the bird collider.** It is drawn 0.8
  world units across against a `2 * BIRD_RADIUS` of 0.7, deliberately, and
  nothing asserts the relationship — so art that grew to twice the collider
  would look wrong and pass. Breakout closed the equivalent gap
  (`every_sprite_covers_the_collider_it_stands_for`, which holds all forty
  bricks, the paddle and the ball to their colliders exactly); flappy's is
  harder only because the bird is deliberately _not_ exact, so the assertion has
  to be a stated ratio rather than an equality.

- **Nothing checks breakout's clear colour against the court's interior.**
  `art::SURROUND` and `field.crpix`'s `f` are two hand-written sRGB values that
  have to read as different surfaces; a change to one is invisible to the tests.
  Low stakes — it is a look, not a behaviour — but it is the one number in
  breakout's art with nothing holding it.
- **Nothing has run the fullscreen toggle against a real compositor.** The
  Wayland and X11 backends already implement `Shell::set_mode` and report an
  effective mode back, and the samples' F11 path is tested end to end against
  `HeadlessShell` — including the refusal case, where a windowed configure
  answers a borderless request. What has _not_ happened is a human pressing F11
  under a real Wayland compositor or an X11 window manager and confirming that
  the window covers the monitor, that the swapchain resizes with it, and that
  `mode_request_honoured` goes true. The development machine for this slice had
  neither `DISPLAY` nor `WAYLAND_DISPLAY`. A tiling window manager is the
  interesting case: it is expected to refuse, and the sample is expected to say
  so rather than pretend.
- **The browser's fullscreen path has never been exercised.**
  `web/tools/browser-e2e.mjs` covers focus and pause in a real browser — it
  blurs the canvas, checks the status becomes `STATUS_PAUSED` and that the HUD
  heartbeat stops, and that Escape brings it back — but not F11. Headless
  Chromium's fullscreen behaviour under Xvfb is its own question, and
  `Input.dispatchKeyEvent` is not a user gesture for the purposes of
  `requestFullscreen`, so the check would need `Browser.setPermission` or a
  headed run to mean anything. What is covered instead is the seam:
  `crcbl-shell`'s `fullscreen_is_the_pages_answer_not_the_engines_request` and
  `the_mode_is_right_whichever_order_the_resize_and_the_change_arrive_in` drive
  `__crcbl_web_fullscreen` directly, and `check-exports.mjs` confirms the symbol
  ships and that the shim calls it.
- **Nothing has looked at the pause menu.** It is asserted as draw-list strings
  — the `PAUSED` heading and the first hint line reaching the UI pass — and no
  human or golden image has confirmed it is legible over either game's art, or
  that the full-frame dim reads as a dim rather than as a bug. It is
  deliberately the crudest possible version, behind `draw_pause_menu`, because
  the next slice replaces it.
- **The changelog starts mid-project.** `CHANGELOG.md` covers changes from
  2026-08-01 onward; everything before it is in `git log` only. Worth doing at
  the first tagged release, or not at all — there are no releases yet for a
  reader to be missing entries from.

## What `crcbl-phys` owes, found by writing asteroids

`apps/asteroids` is the P6 physics slice's first consumer. Two of the questions
P6 left open are answered below and no longer open; the rest are what building
against the crate turned up.

**Answered — the wrap's broadphase rule.** P6 left "when is a move a teleport"
to whoever wrote the wrap. Asteroids' `teleport` (in
`apps/asteroids/src/game.rs`) chose: **a wrap is a teleport, and a teleport is a
remove-and-re-insert**, done by calling `PhysicsSystem::set_collider` again,
applied uniformly to everything in the broadphase with no distance threshold —
"did the position change discontinuously" is what a wrap knows and a threshold
would only guess at it.

**Corrected — the wrap is not a correctness bug.** The old entry here said a
teleported body "leaves its ancestors' bounds stretched across the whole field",
implying collisions break. They do not, and this was checked by falsification:
swapping `set_collider` for `set_transform` in that function leaves the whole of
`apps/asteroids`' 49-test suite green, because `Bvh::update_aabb` refits every
ancestor on the way to the root and a stretched ancestor is a conservative
_superset_ — bigger than it should be, never smaller, so it prunes nothing. What
it costs is **tree quality**, not answers.

- **A consumer cannot see the cost it is being asked to avoid.** `Bvh::depth` is
  public; `PhysicsWorld` exposes no `depth()`, no node count and no `&Bvh`, so a
  game has no way to measure whether its teleport rule helps. The rule above is
  therefore chosen on the argument and not on a measurement. Either expose a
  `PhysicsWorld::broadphase_stats()` or accept that the claim stays unverified.
  Ties into the missing benchmark below.

- **`DampingForce` has no per-entity route and `ThrustForce` does.**
  `ThrustForce::world_force` is public precisely so a game can thrust one entity
  among a field of rocks through `PhysicsSystem::apply_force`, because a force
  _provider_ is global. `DampingForce` and `DragForce` have no equivalent, so
  asteroids re-implements `-k·v` and the `mass/dt` clamp by hand in
  `damping_force`. It is a faithful copy —
  `the_hand_rolled_damping_is_the_engines_own` checks it against
  `DampingForce::apply` directly, including at tick rates coarse enough to reach
  the clamp — but it is a second copy of a physics model in the workspace. Fix:
  give both a `world_force`-shaped method, or give `PhysicsSystem` per-entity
  providers.

- **There is no "what does entity E overlap" query.**
  `PhysicsSystem::overlap_sphere` takes a free centre and radius. An entity that
  is only ever the _subject_ of overlap tests therefore has no reason to be in
  the broadphase at all — asteroids' ship carries no collider, because a leaf no
  query is allowed to return would have needed filtering back out of every
  result by entity id. That is fine here and will not be for a game where two
  things test against each other. What is wanted is an entity-shaped overlap
  with an exclusion list; the same exclusion list is what `sweep_sphere` needs
  and what breakout and flappy both work around by removing the sweeper's own
  collider and putting it back.

- **`PhysicsSystem::overlap_sphere` still fabricates its `ShapeHit`.** `t: 0.0`,
  `normal: DVec3::Y`, `started_inside: true` for every result. Asteroids only
  asks _whether_ anything is there, so it discards the hit outright — which
  means the type is promising a contact that no caller in the workspace can use.
  Either compute a real deepest-point normal or change the return type to entity
  ids. `PhysicsWorld::overlap_sphere` underneath is honest.

- **Segment CCD earns a named method, and still has none.** P6 said "decide when
  asteroids writes it". Asteroids wrote it (`sweep_bullets`), and the verdict is
  yes: `Segment { start: pos - vel * dt, end: pos }` then `sweep_sphere` is the
  same six lines in every game that fires anything, and getting `dt` or the
  order wrong is silent —
  `a_bullet_that_crosses_a_rock_within_one_tick_still_hits_it` goes red under
  exactly that mistake. Wanted: something like
  `PhysicsSystem::sweep_body(entity, dt, exclude)` that reads the body's own
  velocity and radius. Not blocking; three samples in, it is a pattern.

- **Rotational dynamics are absent.** `Transform` carries a `DQuat` and
  `ThrustForce` reads it, but there is no angular velocity, no torque and no
  quaternion integration: `RigidBody` has `velocity` and `force_accum` and
  nothing angular. Asteroids' ship integrates its own heading in `turn_ship` and
  writes it through `set_transform`. That is right for this game — a turn rate
  is a constant, not a physical response — and wrong for the inertia tensor the
  design doc describes. Whoever needs real torque adds `angular_velocity`,
  `torque_accum` and an inertia term to `RigidBody` and a rotation step to
  `SemiImplicitEuler`.

- **No benchmark, and no rebuild policy.** Churn cost was measured as tree
  _depth_, not as time: the claim "insert/remove beats a rebuild" is an
  algorithmic one (one root-to-leaf path against `O(n log n)`) and was not
  timed. There is also no policy that ever rebuilds a churned tree — the AVL
  bound is what makes one unnecessary, but a bulk build still produces a tighter
  tree by surface area than incremental insertion does, and nobody has measured
  the query-cost difference between the two. The horde sample (P8, 10k bodies)
  is where that stops being academic.

## `GameModule::tick` runs after the ECS sweep, so a game's destructions lag

`crcbl_ecs::World::tick` runs the schedule and then `sweep`s the deferred
destruction queue. `crcbl_server::Server::tick` calls `world.tick()` **and
then** `module.tick(&mut world)`. So every entity a `GameModule` despawns sits
in the pool for one more tick before the pool lets go of it — and a game reading
`World::entity_count()` between ticks sees a count that is high by however many
things died last tick.

Found by asteroids, whose leak test compares `entity_count()` against
`1 + rocks + bullets` on every tick and failed immediately; it now adds
`World::dead_queue_len()` to the sum. `apps/flappy`'s equivalent test asserts a
`<=` ceiling, which tolerated this without noticing it.

Two possible fixes, and the choice is the engine's:

- **Sweep after the module**, i.e. `Server::tick` calls `world.sweep()` between
  the module and `emit_snapshot`. Arguably more correct anyway: today's snapshot
  is emitted while entities the module destroyed are still in the pool.
- **Leave it and document it**, and have `World::entity_count` grow a sibling
  that excludes the queue, so a consumer is not obliged to know.

Not worked around in asteroids beyond making the test honest. **Horde hits it
harder**: its leak invariant is checked on every tick of a soak, and the queue
is non-empty on any tick something died — which at a hundred spawns a second is
most of them.

## What `crcbl-phys` owes at scale, found by writing horde

`apps/horde` runs `N` broadphase overlap queries per tick — one per enemy, for
separation — plus one for contact damage and one for aiming, so it is the first
consumer where the _per-query overhead_ rather than the query's answer is the
cost. Provisional numbers and their conditions are in
`docs/plan/sample/03-horde.md`; both entries below sit in front of them and
neither was taken, because either is an API change to `crcbl-phys` and this
slice was the sample.

- **`overlap_sphere` returns an owned `Vec`, so a game that queries per body
  allocates per body per tick.** `PhysicsSystem::overlap_sphere` and
  `PhysicsWorld::overlap_sphere` both build and return a `Vec`, and
  `Bvh::traverse_aabb` builds another one underneath, so horde's `steer_enemies`
  does **two** heap allocations per enemy per tick and drops them immediately.
  At the plan's 10 000 that is 1.2 million allocations a second doing nothing.
  Wanted: a callback or `&mut Vec` form —
  `overlap_sphere_into(centre, radius, &mut out)` — through all three layers, so
  a caller can hoist one buffer out of the loop. Not measured against the
  alternative, because there is no alternative to measure yet; it is named here
  as the first thing to try rather than as a proven cause.

- **`PhysicsSystem` has no `body_mut`, so writing a velocity is a `HashMap`
  insert.** `body()` hands back `&RigidBody`, and the only writer is `set_body`,
  which does `bodies.insert(entity, body)` and then
  `transforms.entry(entity).or_insert(…)` — two hash operations to change one
  `DVec3`. A game whose agents _choose_ their velocity rather than having one
  integrated onto them (horde's enemies are all `RigidBody::new_kinematic`) does
  that `N` times a tick. `apply_force` does not help: on a kinematic body it is
  a no-op, because `inverse_mass` is zero. Wanted:
  `body_mut(entity) -> Option<&mut RigidBody>`. It is three lines and was not
  taken here only because it is a public-API addition to a crate this slice had
  no other reason to open.

- **Steering is embarrassingly parallel and there is nothing to run it on.**
  Horde's separation pass reads positions, queries, and writes velocities —
  nothing it writes is read by the broadphase, so the pass is order-independent
  by construction and every enemy could be done on a different thread. There is
  no `crcbl-jobs` (P8) and no parallel ECS schedule, so it is a `for` loop. This
  is the entry the roadmap already predicted; it is repeated here with the
  evidence that the _shape_ is right, which the roadmap could not know — and
  with a number: **14.66 ms a tick at ten thousand spread, 84.09 ms converged**,
  against a 16.67 ms budget. Sixteen cores is the difference between the sample
  hitting its target and missing it by 5×, and there is no shared mutable state
  in the pass to stop them.

- **A broadphase query costs what its _answer_ costs, so the tick's cost tracks
  local density rather than entity count.** The same ten thousand enemies cost
  14.66 ms a tick spread over the arena and 84.09 ms after eight seconds of
  converging on the player — measured, both columns, in
  `docs/plan/sample/03-horde.md`. This is not a complaint about `crcbl-phys`; it
  is the fact any budget stated in "N agents" is wrong about, and it is why
  18a's provisional 8–9k figure was both too optimistic (it never let the crowd
  converge) and taken on a fixture that at ten thousand described a field larger
  than the arena. **Anything that quotes a per-agent cost for this crate has to
  say what the neighbourhoods looked like.**

- **The neighbour sum's order is the BVH's traversal order, and horde chose to
  live with it.** Floating-point addition is not associative, so the separation
  vector depends on the order `overlap_sphere` returns neighbours in. That order
  is deterministic — the tree is a pure function of the insert/remove sequence,
  which is a pure function of the seed and the script, and
  `the_same_script_replays_bit_identically` covers it — but it is deterministic
  _because of the tree_, not independently of it. Sorting each neighbourhood by
  entity id would make it independent, at the price of a sort per enemy per
  tick. **Declined** for that reason. It becomes a real question the moment the
  tree's build order stops being reproducible — a parallel insert, or a rebuild
  policy that depends on timing — and whoever adds either should read this
  first.

- **A sphere overlap of radius `R` returns everything within `R + r_b`, and
  horde depends on that exactly.** Not a complaint — it is what makes
  `separation_query_radius` `r_self + slack` rather than
  `r_self + max_enemy_radius() + slack`, which at a brute's 0.85 would nearly
  triple the area a grunt searches. It is written down because it is an
  undocumented consequence of `overlap_sphere` being shape-aware rather than an
  AABB test, and nothing in `crcbl-phys` says so.
  `the_separation_query_radius_is_exactly_the_neighbourhood` is the guard, in
  the consumer, where it does not belong.

## A projectile swept on the tick it is fired is swept backwards through its own muzzle

Segment CCD reconstructs `prev` as `position - velocity * dt`. For a projectile
created _this_ tick that point is one whole step behind the muzzle — through the
body that fired it and out the other side — so the sweep covers ground the
projectile never travelled.

`apps/asteroids/src/game.rs` fires before it sweeps, so it has this: a bullet
can hit a rock sitting behind the ship on the tick it leaves the gun. It is 0.4
of a unit at 60 Hz on a 32-unit field, hidden inside the ship's own hull, and no
asteroids test looks for it — which is why it is a note here rather than a bug
report. It scales with `1/tick_rate`: at `--tick-hz 4` it is six units.

`apps/horde` fires **after** the sweep instead, so a bolt's first sweep is its
first real step and `start` is exactly the muzzle;
`a_bolt_is_never_swept_over_ground_it_did_not_travel` pins it, at 4 Hz where the
phantom segment is 7.5 units long. Asteroids was not changed to match: the fix
is a three-line reorder in a sample this slice was not otherwise touching, and
it wants its own before/after on asteroids' own suite.

## What horde still owes

S3 is done — the core loop, the art and progression, and now audio, the longest
run, the browser demo and the scale measurement. `docs/plan/sample/03-horde.md`
carries the numbers and their conditions; this is what was raised and not
finished. Entries the measurement closed have been deleted rather than
annotated.

- **Do not put the autostart back.** This sample shipped without a start screen
  on purpose — its board is empty at `t = 0`, so a waiting state is a blank
  arena with a prompt on it, sample rules 4 and 11 do not require one, and
  adding `WaitingToStart` churned the suite exactly as predicted. **The user
  played it and asked for the screen**, which reverses that call for good: a
  demo that starts taking hit points off a player who has not looked at the
  window yet is worse than a blank arena, and four samples that open the same
  way are worth more than one clever exception. `GameState::WaitingToStart`
  short-circuits `run_tick`; `restart` lands on the title screen rather than in
  play, so `TRY AGAIN` takes two presses, the same as asteroids and flappy. The
  argument is preserved in `GameState`'s own docs and in `crate::menu`'s header
  — it is recorded so it is not re-derived, not so it can be re-applied.

- **`--prefill` starts its own run, and that coupling is not obvious.**
  `Loop::assemble` queues a start edge when `options.prefill > 0`, because the
  scale fixture would otherwise measure a `run_tick` that returns on its second
  line. It is one call beside `Game::stage_field` and
  `a_prefilled_run_does_not_wait_at_the_title_screen` holds it. Anything else
  that stages a board before the first frame — a replay header, a future demo
  mode — has to do the same or it will measure nothing and say it measured
  everything.

- **The plan's exit criteria are internally inconsistent and need rewriting, not
  answering.** "10 000 enemies at 60 Hz tick" is true of a crowd spread over the
  arena (14.66 ms of a 16.67 ms budget) and false of the same crowd converged on
  the player (84.09 ms). The difference is a factor of 5.7 at a fixed count,
  because separation is a broadphase query whose cost is the size of its answer;
  a horde converges by construction, so the second number is the one the game
  spends its time at. Whoever owns the criterion has to say **which crowd**.

- **"Playable and mildly fun for 5 minutes" cannot be true of this arena at the
  plan's count.** Ten thousand enemies in 96 × 72 units is 0.82 units apart,
  several inside `PLAYER_RADIUS` on frame zero, and contact damage is a rate
  summed over everything touching — so `--prefill 5000` and above kills the
  player in under a second. A default run, spawner only, dies at about 24
  seconds with 46 things on the field. Two ways out and neither is obviously
  right: **a bigger arena** (the density falls as the area grows, and the follow
  camera and `clamp_to_arena` already handle any size, but a 300 × 225 arena
  changes what the game _is_ — the walls are what makes kiting finite), or
  **admit the count is a benchmark target** and let the exit criterion carry two
  numbers, one for the budget and one for the game. Nobody has decided.

- **`SpriteRenderer` has no batch count, so the sample's central claim is
  checked against a copy of the rule.** `art::batches` counts runs of
  consecutive sprites naming one sheet, which is exactly what
  `crcbl_render::sprite_pass::batch` does; `SpriteRenderer` exposes
  `sheet_count()` and nothing else.
  `a_batch_is_a_run_of_one_sheet_and_not_a_distinct_sheet_count` pins the mirror
  at `A A B A` = 3 — the case a distinct-sheet count gets wrong, and one this
  game's own frames cannot produce, so it had to be written synthetically — but
  nothing would notice the engine's rule changing underneath. Wanted: a
  `batch_count()` beside `sheet_count()`, returning
  `self.batches[self.frame].len()`. Three lines; not taken because `crates/` was
  outside the slice's write scope.

- **The CPU cull is still per-sprite and still `N` per frame**, and it is now
  measured: 28 µs at ten thousand, of a 16.67 ms budget. That is the work P7's
  GPU culling exists to delete, and the measurement says deleting it is worth
  0.17 % of a frame to this sample. Keep it as the reason P7 exists for _other_
  scenes; it is not the reason it exists for this one.

- **`crcbl-audio` has no voice limit, no priority and no stealing.** Still true
  after the mixer-adoption slice, which deliberately left it there.
  `apps/horde/src/audio.rs` caps itself at `MAX_VOICES` = 16 and refuses the
  newest voice, counting refusals in `Audio::dropped()`. Refusing the newest is
  the crudest answer that is honest and it is audibly wrong in one case: a
  player's _death_ cue can be refused by sixteen kill cues raised on the same
  tick. Wanted in the crate: a voice budget with a priority, so an important cue
  steals the oldest cheap one. Nothing shows `dropped()` yet — it is on `Audio`
  and not on the debug panel.

  One detail changed with the adoption and is worth knowing before anyone moves
  the cap into the crate: horde now reads `Mixer::voice_count` and then calls
  `Mixer::play`, two lock acquisitions where the hand-rolled queue held one
  across both. Only the game thread adds and only the audio thread removes, so
  the count can be stale **low** and never stale high — the cap can refuse a cue
  that had just been made room for, and can never let the count past
  `MAX_VOICES`. A cap inside the crate would not need the two-step at all.

- **Nothing has listened to the five cues**, on any device. They are synthesised
  deterministically from a fixed seed, so a golden buffer is possible and there
  is not one. What the tests assert is that each cue fires, that it carries the
  position of the thing that raised it, that the listener is the player rather
  than the origin, and that the level cue actually sweeps in pitch. No test can
  tell a good kill sound from a bad one.

- **The HUD line can still outgrow its backdrop at extreme settings.**
  `the_hud_fits_the_panel_it_is_drawn_on` measures both lines through the real
  `FontAtlas` at a stated worst case — a five-minute run at the shipped enemy
  cap, level 18, 2 048 kills — and requires them inside `HUD_PANEL_RIGHT`. It
  does **not** bound `--max-enemies 10000` with a twenty-minute soak behind it:
  five-digit fields are wider than the panel and the text would run off the end
  of it, which is what the browser gate's canvas capture caught the last time
  the panel was too narrow. A real fix is a HUD that measures itself and sizes
  its own backdrop, which is a `crcbl-ui` widget rather than a `DrawList::rect`.

- **Nothing enforces that the arena is a plane.** Positions are `DVec3`,
  everything the game produces sits at `z = 0`, and `clamp_to_arena` passes `z`
  through untouched. A body given a non-zero `z` would separate in depth and
  never be brought back — which a test fixture using `DVec3::splat` did, and
  which is how this was noticed. Either clamp `z` too or make the fact a type.
  Not a live bug: no production path can produce one.

- **The horde does not avoid the walls, it is pushed into them.** Seek is a
  straight line to the player and separation knows nothing about the arena, so a
  crowd chasing a player along an edge piles into it and the clamp holds it
  there. It reads acceptably and it is not pathfinding — which is a hard
  non-goal — but "walk around the obstacle" is the first thing a player will
  expect if props ever land in the arena.

- **Contact damage has no invulnerability frames**, by choice: it is a damage
  _rate_ summed over whatever is touching, so a stack of enemies is worse than
  one and there is no per-enemy timer on the hot path. The consequence is now
  measured rather than predicted — see the density entry above — and it is what
  makes the plan's count unplayable in this arena.

- **The spawn ring is relative to the player and clamped into the arena**, so a
  player standing in a corner gets enemies materialising on the wall beside them
  instead of walking on from off screen. Rejecting and re-drawing the angle
  would fix it and would no longer be a pure function of the index, which is the
  property the determinism suite rests on; the honest fix is to pick the arc
  that is inside the arena rather than to retry.

- **Nothing pulls a gem towards the player.** `Upgrade::Magnet` widens the
  radius the collection query runs at, so a gem inside it is banked on the tick
  it comes into range and one outside it is not. The genre's version drifts the
  gem to the player, which reads far better and which would be `P` steering
  updates a tick on top of the `N` the sample measures. Left out deliberately:
  the point of this slice's pool is that each upgrade is one line.

- **A gem that is never collected is lost, silently.** `MAX_PICKUPS` is 512 and
  a kill on a full field drops nothing; `Game::pickups_dropped` counts the
  refusals and nothing shows them. It is bounded and deterministic, which is
  what it was for, and a player kiting away from a heap of loot in a long run
  will not be told why their level-ups stopped. A HUD line, or dropping the
  _oldest_ gem instead of refusing the newest, would both fix it.

- **The level-up screen has no way out but forwards.** There is no "skip", and a
  choice out of range is ignored, so a run that reached `LevelUp` stays there
  until one of the three digits is pressed. The loop's Escape still pauses over
  it and the death menu cannot be reached from it — nothing can kill the player
  while the field is frozen, so this is not a soft-lock, but it does mean a
  browser demo left on the level-up screen looks stopped. `browser-e2e.mjs`
  watches the once-a-second `[HUD]` heartbeat, which keeps firing, so the gate
  itself is fine.

- **The upgrade pool is repeatable without limit.** `RapidFire` has a floor
  (`FIRE_COOLDOWN_FLOOR`) and the other five do not, so a very long run has an
  unbounded weapon range, walk speed and hit-point ceiling. It is a five-minute
  game and nobody has played it for twenty; caps are a balance decision, not a
  bug, and they are not there.

- **Enemies do not turn to face anything.** Every silhouette is deliberately
  non-directional — a lump, a four-legged X, a horned slab — so no sprite
  rotation is needed and no `atan2` runs per enemy per frame. It is the right
  trade at 10k and it does mean the crowd has no sense of heading.

- **Not measured, not reviewed: the windowed native path.** It is compiled and
  never run — there is no display in this environment — so the follow camera,
  the sprite pass, the three menus and the HUD layout have been checked by test,
  by argument, and now by a **browser**: the gate's canvas capture at 26/26 is a
  picture of the real game, and a human has looked at it. What has still never
  been seen is the _native_ window, and the fullscreen toggle against a real
  compositor is the same gap the other three samples carry.

- **Every scale number was taken on an offscreen image ring, not a swapchain.**
  `--headless` gives `crcbl-vk` a `SurfaceTarget::Offscreen` rotation of images,
  which exists precisely so that it is the same acquire/record/submit/present
  path — but it is not a windowed present, it is not vsynced, and it is 960
  × 720. A windowed 1440p run would raise the sprite pass's fill by about four
  times, which on a 0.023 ms pass is still nothing, and nobody has taken it.

- **There is no Tier B / browser scale number.** The exit criteria ask for one
  ("Tier B/wasm gets its own smaller recorded budget") and the only browser this
  repository can drive is Chromium's SwiftShader under Xvfb, which measures a
  software rasteriser. It needs a machine with a real browser GPU and a way to
  read `PassTimers` out of a wasm build — the second of which does not exist:
  the demo has no way to report its frame timings to the page.

- **Nothing checks that a `.crpix` texel lands on a whole screen pixel.** At
  `TEXELS_PER_UNIT` = 20 and a 720-pixel-high view of 28 world units, one texel
  is 1.286 screen pixels, so `SampleMode::Pixel`'s nearest sampling drops and
  doubles rows as the camera moves. Every sample has this and none of them
  addresses it; the fix is an integer-scaled render target, which is a renderer
  feature nobody has asked for.

## What asteroids itself still owes

S2 is done — simulation, art, audio, persistence and the browser demo. What is
not:

- **No thrust flame still, and the audio slice was where it was going to land.**
  The previous entry here said "do it with the audio slice, where the thrust cue
  lands anyway", and the audio slice did not: `RenderState` still carries no
  thrust intent, so the ship draws one frame whatever it is doing and a player
  hears the engine without seeing it. It is two rows of `assets/ship.crpix`, a
  `bool` on `RenderState` set from `Intent::thrust`, and a frame index in
  `art::Scene::build`. The `bool` now exists on the simulation side —
  `GameLogic::thrusting`, mirrored onto `Game::thrusting` — so what is left is
  carrying it onto `RenderState` and picking the frame. The cue timer that used
  to be suggested as the flame's clock is gone; a flame that flickers wants its
  own, and the frame's alpha is the honest source.
- **No golden buffer for the cues.** The three sounds are synthesised
  deterministically — `audio::noise` runs splitmix64 from a fixed seed — so a
  golden buffer is _possible_, and there is not one. What the tests assert is
  that each cue fires, that it carries the position of the thing that raised it,
  and that the explosion decays and is not a tone. Nobody has listened to the
  result on a real device and no test can tell a good explosion from a bad one.
- **Positions are not interpolated, and the wrap is why.** Every angle asteroids
  draws is lerped across the frame's alpha; every _position_ is the last tick's,
  so a rock at 60 Hz on a 144 Hz display moves in sixtieths. The fix is not
  "lerp those too": the playfield wraps, so a body that crossed an edge has a
  previous position a whole field away and interpolating it would fly it back
  across the screen. What it needs is for `RockView` / `BulletView` / the ship
  to carry the previous position **and a flag saying it teleported**, set by
  `teleport_if_outside`, with the renderer snapping rather than lerping on that
  tick. That is a change to what the simulation publishes, not to how it is
  drawn, which is why it was not folded into the art slice.
- **No visual for a shot hitting a rock.** The explosion is audible and not
  visible: a rock disappears and two smaller ones appear, with nothing between.
  Particles are a hard non-goal in `docs/plan/sample/02-asteroids.md`, so
  whatever this is, it is not that — a two-frame flash sprite at the hit
  position is the cheapest thing that would read.
- **Rocks that straddle an edge are drawn once, not twice.** A large rock is 3.4
  units across on a 32-unit field, so for about a tenth of a second per crossing
  half of it is missing rather than appearing on the far side. Drawing a ghost
  copy at the wrapped offset is a handful of extra sprites and no simulation
  change; not done because the slice was scoped to the art and this is a
  rendering rule the other two samples have no precedent for.
- **The 10-minute soak in the exit criteria was not run.** What runs in CI is
  `hundreds_of_spawns_and_deaths_leak_nothing`: 18,000 ticks (five minutes of
  simulated play), 337 rocks spawned, 1,221 bullets fired, six waves cleared,
  checking the entity and collider accounting on **every** tick. Ten minutes of
  wall-clock soak with the inspector open, and the "no stale-handle panics with
  entities selected as they die" criterion, both need the entity inspector,
  which this sample does not use yet.
- **The overlap query does not know about the seam.** Ship-versus-rock is a
  single `overlap_sphere` at the ship's position, so a ship straddling an edge
  does not see a rock straddling the opposite one until one of them has wrapped.
  A full answer queries up to four offset positions. Deliberately not done: it
  costs four broadphase queries a tick to fix a one-tick artefact at a boundary
  both bodies cross constantly, and no test could tell the difference without
  being written to.
- **No golden image covers a single asteroids pixel.** Weaker than it was: the
  browser gate now loads the demo in a real Chromium, opens a WebGPU device and
  reads the canvas back at 26/26, so "the frame is not blank, not one flat
  colour and changes between frames" is checked — 89 distinct colours across a
  959×463 canvas on the SwiftShader adapter. What is still unchecked is whether
  it is the **right** picture, and in particular whether a rotated
  `SampleMode::Pixel` sprite looks right on a real driver.
  `crates/crcbl-vk/tests/vk_e2e.rs` has sprite goldens including a rotated one,
  so the shader path is covered; the game's own frame is not. There is also no
  display in the build environment, so the _windowed_ native path is compiled
  and never run. The art was checked by eye against the baked PNGs, and that is
  the honest report of it.
- **Tuning constants are compiled in.** The plan's milestone 3 wants them from a
  data file after stage 6. Every one of them is a `pub const` in `game.rs` with
  its reasoning written beside it, which is the form that survives being moved
  into a file.

## Deferred decisions

Questions that came up mid-slice and were answered by judgement rather than by
asking. Each is the question, the answer taken, and **what would change it** —
because the point is that a later reader can reopen one cheaply instead of
rediscovering that it was ever a question. An entry here is not a complaint
about the answer; most of these are probably right.

Distinct from _Considered and declined_ below, which is for ideas rejected on
their merits and expected to stay rejected. These are answers taken under
uncertainty.

- **Should `SoundBank` hold `Arc<[AudioSample]>` rather than `Vec`, so
  `create_voice` stops copying the sound?** _Yes, and it is why horde adopted
  the bank at all._ `SoundBank::create_voice` cloned the whole sample buffer per
  voice, which at horde's cue rate — up to about forty a second, each an
  allocation the size of the sound — was the one measured reason to keep the
  hand-rolled `Arc<Sound>` bank instead. Changing `Voice::data` to
  `Arc<[AudioSample]>` and `create_voice` to `Arc::clone` deletes the reason,
  and `Voice::new(Vec)` still compiles for every existing caller.
  `a_bank_shares_one_buffer_with_every_voice_it_makes` in
  `crates/crcbl-audio/src/mixer.rs` pins it on `Arc::strong_count`. **What would
  change it:** a bank that wants to hand out _mutable_ sample data, which
  nothing does.

- **Should `AudioStream::open` have kept taking `impl AudioSource` by value, or
  changed to `Arc<dyn AudioSource>`?** _Kept — the sharing went into a blanket
  `impl<T: AudioSource + ?Sized> AudioSource for Arc<T>` instead._ Changing
  `open`'s signature would have broken every existing caller, including the
  `wasm32` `web::install` path and the crate's own tests, for a case a blanket
  impl serves without touching any of them:
  `AudioStream::open(Arc::clone(&mixer))` now type-checks and a non-shared
  source still moves in as before. The cost is one redundant `Arc` layer on the
  shared path — `open` wraps whatever it is given in an `Arc` of its own — which
  is a pointer chase per block, not per sample. **What would change it:** a
  source that needs to be reached from the stream _and_ from two other places
  with different types, where the double `Arc` stops being the only wart.

- **Should the voice cap have moved into `Mixer` while the samples were being
  migrated?** _No — horde keeps `MAX_VOICES` and its refuse-newest policy._ The
  crate has no cap, no priority and no stealing, and the honest version of that
  feature is a voice budget with priorities so a death cue can steal a kill cue,
  not a bare count. Shipping the bare count in the engine would have frozen the
  crude policy as the crate's answer and taken the evidence for the good one
  with it, since horde's `Audio::dropped()` is the only measurement of the
  problem anyone has. **What would change it:** a second sample needing a cap,
  which would make it a pattern rather than one game's answer.

- **Should the samples' spatial assertions read the mixer, or the rendered
  audio?** _The mixer, through `Mixer::voice_mixes`._ Rendering a block and
  measuring left against right is the stronger observable and it was tried: it
  races the null stream's polling thread, which is draining the same mixer every
  five milliseconds and will have eaten an unpredictable prefix of any voice by
  the time the test looks. The gain-reaches-the-output half is checked once, in
  the engine, where a test can own a `Mixer` with no stream attached —
  `set_mix_re_aims_a_voice_that_is_already_playing`. **What would change it:** a
  headless `Audio` that opens no stream at all, which would make the render
  check deterministic in every sample.

- **Where does the menu art live?** Taken: **`crates/crcbl-render/assets/`**,
  baked by that crate's own `build.rs`. `apps/*` cannot depend on each other, so
  per-sample art is the same window authored three times and three games that
  look like three engines. The rejected alternative was a shared `assets/`
  directory both build scripts reach into: it shares the `.crpix` and nothing
  else — each script still bakes, each `art.rs` still loads, each game still
  writes the layout — and it puts a `../../..` path outside a package's own
  tree, which cargo does not track for rebuilds the way it tracks a package's
  files. It also gives `crcbl-vk`'s suite nothing, because that crate cannot see
  `apps/`, so the golden would be a picture of a replica. _Changes it_: art that
  is genuinely one game's. A sample that wants its own frame should author it
  under its own `assets/` and pass its own `MenuArt`; the shape for that is a
  constructor beside `MenuArt::register` taking a `Sheet`, not a fork of this
  one.

- **What size is the menu drawn at?** Taken: **the largest whole scale in 1..=4
  whose panel fits inside 90% of the framebuffer**, a pure function of the
  extent and the menu's own contents (`Menu::layout`). Whole numbers because the
  art is pixel art and a fractional scale puts a nine-slice corner on a half
  pixel; a fit rather than a constant because a fixed size is either lost on a
  4K screen or off the bottom of a 1440×400 canvas. _Changes it_: a settings
  screen with a UI-scale slider, at which point the scale is the player's and
  `layout_with` is already the entry point that takes one.

- **Does the menu shadow a key a sample had bound?** Taken: **yes, once** —
  flappy's `ArrowUp`, which is the _second_ binding of its flap action beside
  `Space`. The three menu keys (Up, Down, Enter) are the same three in every
  sample, for the reason F3, Escape and F11 are; two of the three are free in
  every game and this one is not. Space is never shadowed, is what the HUD has
  always named, and is printed on every button that flaps. The keys are consumed
  only while a menu is on screen. _Changes it_: a sample that binds Enter or the
  vertical arrows to something a player uses _while a menu is up_ — which today
  is nothing, because a menu is only on screen when the simulation is stopped or
  waiting. **Asteroids is the second, and it shadows `ArrowUp` too** — its
  second thrust binding, beside `KeyW`, which is not shadowed. Same trade, one
  sample later, and it costs less there: a menu is up only on a frame the ship
  is not being flown.

- **Does the world keep drawing behind a menu?** Taken: **yes, and it is
  dimmed** by a scrim sprite the menu's own pass draws. A frozen screenshot
  would need a captured frame and a second code path; a menu with nothing behind
  it loses the player's place. The scrim is a _sprite_ and not a `DrawList`
  rectangle because the UI pass runs after the sprite pass, so a UI-pass scrim
  would dim the menu's own frame along with the game. _Changes it_: a menu that
  wants the game genuinely stopped in the background — a settings screen over a
  paused multiplayer session, where the world is still ticking and the motion is
  a distraction.

- **Does a looping ping-pong replay its end frames?** Taken: **no.** A looping
  ping-pong's period is `2n - 2` — four frames run `0 1 2 3 2 1` and then `0`
  again — while a one-shot is `2n - 1`, precisely because that trailing `0` is
  no longer the next cycle's first, and an out-and-back that stopped on frame 1
  would look truncated. `Clip::steps` and `Clip::step` in `crcbl-sprite`.
  _Changes it_: art that wants a beat held at an end — a wing pausing at the top
  of its stroke. Today that is spelled by giving the end frame a longer `hold`,
  which works and is per-frame; if it turns out to be the common case rather
  than the exception, the answer is a hold on the return leg, not a global flag.

- **Does `reverse` reverse a clip's holds too?** Taken: **no** — a hold belongs
  to the frame it holds, not to the position in the list, so a reversed clip's
  tick pattern is the forward one read backwards. Documented on `Clip::step`.
  _Changes it_: a consumer wanting the reversed clip to have the same
  tick-by-tick timing _profile_ as the forward one (slow-then-fast staying
  slow-then-fast). Nothing has asked, and the current rule is the one that makes
  a frame's timing a property of the frame, which is easier to author against.

- **What does a nine-slice do when the target is smaller than its corners?**
  Taken: **the fixed bands shrink in proportion and the stretched band
  vanishes.** `NineSliceSource::expand`. The two alternatives were both worse:
  _refusing_ — emitting nothing, or clamping the target up to the minimum —
  makes a pipe squeezed below its caps either disappear at one size and not
  another or spill outside the rectangle it was handed; _letting the corners
  overlap_ inverts the middle band, which with no backface culling rasterises a
  mirrored quad rather than nothing, and double-blends the overlap. Shrinking
  keeps the three properties that matter more than corners staying literally
  fixed at a size where they arithmetically cannot: the quads still tile the
  target exactly, nothing is drawn outside it, and the picture is continuous —
  at exactly the minimum size this path and the ordinary one agree. _Changes
  it_: a caller that would genuinely rather draw nothing than draw squashed
  corners. That is `NineSlice::fits_in` at the call site, not a change here.

- **Should nine-slice edges tile instead of stretching?** Taken: **no tiling
  mode at all.** Two concrete costs: a tiled band is `ceil(extent / inset)`
  quads rather than one, so the instance count stops being bounded by nine and
  starts depending on how big the thing was drawn — a pipe stretched to a tall
  gap would quietly become hundreds of instances — and doing it in UV space by
  letting `u1` run past 1 needs a repeating sampler, while `SpriteRenderer` has
  exactly one sampler, `ClampToEdge`, shared by every sheet. _Changes it_: art
  whose edge is a repeating motif that stretching visibly smears — a chain, a
  rope, a brick course. Then it is a new mode with its own quad emitter, not a
  flag on `expand`.

- **Teach the UI pass a second texture, or draw button skins as sprites?**
  Taken: **sprites**, and an older backlog entry that called this "blocked on
  the UI pass sampling a second texture" was deleted as wrong rather than
  satisfied. The UI atlas is a single-channel `R8Unorm` glyph _coverage mask_
  sampled into alpha only — every fragment's RGB comes from the vertex colour —
  and a button skin is RGBA colour art. Routing it through would need a second
  bound image in a second format, a per-quad branch between two samplers, a
  UV-carrying draw command `DrawList` does not have, and an RGB path added by
  hand to both tier permutations of `ui.slang`. `SpriteRenderer` already is an
  instanced RGBA pass with alpha blending, and a skinned button is nine sprites.
  _The cost paid_: the caller owns the ordering. `RenderGraph` runs passes in
  declaration order with no topological sort, and both passes load rather than
  clear, so `SpriteRenderer::add_pass` must precede `UiRenderer::add_pass` or a
  skin paints over its own label — enforced by nothing but the order of two
  lines. _Changes it_: a UI element needing colour art _interleaved_ with text
  rather than behind it, which two passes cannot express at any ordering.

- **A fixed backdrop for breakout, or a parallax band?** Taken: **fixed.**
  _(Moved here from Considered and declined — it is a judgement about this
  game's camera, not an idea rejected on its merits.)_ Breakout's camera never
  moves — the field is fixed and the whole of it is on screen — and `Parallax`
  is `(1 − factor) × camera`, so with a camera at the origin every factor
  produces the same offset of zero. A "distant" layer and a world-locked one
  would be the same picture, and a band that scrolled anyway would be motion the
  player has no reason for. `art::Scene`'s two layers are both `Parallax::WORLD`
  and exist for depth ordering, which is the half of a `LayerStack` that still
  means something here. _Changes it_: breakout gaining a camera that moves — a
  screen-shake on a brick break would be the obvious one, and is currently a
  scope violation under the sample's "no juice" cap.

- **Texels-per-unit: fix the engine, or let each sample pick a scale?** Taken:
  **let the convention stand through P4B, fix it before the third sample.**
  `NineSliceSource::expand` takes its insets as target units directly, so a
  6-texel cap is 6 units tall whatever the caller's world is; flappy scaled its
  sprite plane by 20 and breakout by 10, chosen independently from the pipe and
  from the ball. The reason for deferring was scope — the slice was `apps/*` and
  the fix is in `crcbl-render` — not doubt about the fix. _Changes it_: nothing;
  this one is already owed. The work item, with the shape the fix should take,
  is under **The sprite system** above. Recorded here so the deferral itself is
  on the record and not just its consequence.

- **Commit the baked PNGs beside the `.crpix` text?** Taken: **no.** _(Moved
  here from Considered and declined.)_ It would make the build faster and the
  art reviewable in an ordinary diff, and it would create two sources of truth
  for one picture — the one a reviewer reads being the one that is not loaded.
  `docs/specs/crcbl/pix.md` is explicit that `.crpix` is a build input, and both
  samples' `build.rs` keep it that way. _Changes it_: a build where baking is
  slow enough to be felt, or a review workflow that genuinely cannot read
  `.crpix`. Neither is true today — the baker is a strip blit — and the honest
  fix for the second would be rendering `.crpix` in review, not committing PNGs.

- **What phase are the eleven sprite slices?** Taken: **P4B**, by analogy with
  P4A audio, which was the same shape — a subsystem that was not in the original
  phase table, delivered between numbered phases. It is written into both the
  roadmap's status table and its phase table. _Changes it_: a preference for a
  different label. Nothing depends on the string except the roadmap's own
  cross-references and this file.

- **What stays at P10 now that the frame-timing core is built?** Taken: **the
  rest of it.** The core shipped early, out of P10, because both existing
  samples wanted it and two more are planned before P10 — leaving it there would
  have guaranteed a third and fourth per-sample HUD, the shape `web.rs` already
  took twice. What P10 still owes is the rest of `07-ui-debug.md`'s suite
  (inspector, console, culling stats, debug-draw controls, UI inspector) and
  `23-netcode.md`'s netgraph, which is unbuildable before the transport can
  measure itself. _Changes it_: a sample that needs one of those sooner, which
  is the same argument that moved the frame-timing core.

- **How does a module register with the panel — retained list or per frame?**
  Taken: **per frame**, `DebugPanel::add(&dyn DebugModule)` once per system the
  frame actually has, matching the crate's immediate-mode authoring. A retained
  registry would need the panel to hold borrows or `Rc`s of every system that
  reports, which is the plugin framework `07-ui-debug.md` explicitly does not
  want, and it would make "a section appears because the system is present" into
  "a section appears because someone remembered to register and to unregister".
  _Changes it_: a module whose data is expensive enough to want gathering off
  the frame path, which would want a handle rather than a per-frame call.

- **What does the panel's FPS number mean?** Taken: **frames divided by the time
  they took** over a rolling 120-frame window, not the mean of the per-frame
  rates. The two agree only when every frame is the same length: 10 ms and 30 ms
  average to 67 FPS as reciprocals and to 50 FPS as `2 / 40 ms`, and the second
  is what the window actually ran at. 120 frames is two seconds at 60 Hz — short
  enough to react while you are looking at it, long enough to read. _Changes
  it_: wanting a 1%-low figure, which needs the sorted window this deliberately
  does not keep.

- **Which samples are exempt from the pixel-art rule?** Taken: **hud, viewer and
  sparks**, on the ground that each one's _subject_ is something other than
  pictures — a widget gallery, the user's own glTF, and a particle workbench —
  so authored sprite art in front of it would be showing the wrong system. hud
  still authors its button skins as `.crpix` because a skinned widget is a
  widget. Every other sample on the ladder is in scope. _Changes it_: a sample
  arguing itself out, which sample rule 11 requires it to do in its own doc with
  a reason.

- **Is `docs/code-review.md` a record or a description of current state?**
  Taken: **a record**, and left unedited except for a line in its header saying
  so. It is dated 2026-08-01, was added in one commit and never amended, and the
  roadmap already says its findings were fixed across eight commits. Several of
  its findings now describe code that no longer exists — the `paddle_model`
  finding is the clearest, since breakout has no forward pass at all. _Changes
  it_: a decision to keep it live, which would mean re-running the review rather
  than patching the findings that happen to have been noticed.

- **What does a paused frame do to the fixed-tick accumulator?** Taken: **update
  the clock and drain the accumulator without stepping the game.** The three
  candidates only differ after a long pause. _Not calling `update`_ freezes
  `FrameClock::last_update`, so the first update after the pause measures the
  whole of it and the `DEFAULT_MAX_CATCH_UP_TICKS` cap turns it into eight ticks
  in one frame — measured, not reasoned: falsifying the drain that way makes
  `resuming_after_a_long_pause_runs_one_tick_not_a_catch_up_burst` report "ran 8
  ticks" in all three samples. _Updating but not draining_ saturates the
  accumulator at the same cap and lurches identically, also measured. Draining
  leaves only the sub-tick remainder, so the first live frame runs the one tick
  it is owed, and it keeps `render_dt` real so the debug overlay's frame graph
  does not flatline at whatever it read when Escape was pressed. The cost is
  that `FrameClock`'s `TickId` advances during a pause; nothing in any sample
  reads it. _Changes it_: a consumer that does — a networked sample whose tick
  ids have to line up with a server's — which would want an explicit
  `FrameClock::reset` rather than a drain loop.

- **Is pause a `GameState` variant or the app loop's?** Taken: **the loop's.**
  Both samples' `GameState` lives inside `GameLogic`, which the authoritative
  server's `GameModule` mutates from inside a tick and which the client
  replicates; a `Paused` variant there would make the server's state depend on
  which window a player's compositor has focused, and would put a value in
  `Summary::state` that a headless scripted run could reach. Pause is not
  something the simulation does — it is the loop declining to advance it — so it
  is `Loop::paused`, reported through `Loop::is_paused` and `Summary::paused`.
  _Changes it_: a pause the _simulation_ has to know about, which in a
  multiplayer build it would: pausing a shared world is a server decision and
  would be a state on the server, not a client's window losing focus.

- **Does regaining focus resume?** Taken: **no.** A player who clicks back into
  the window would otherwise arrive mid-ball with no warning, and the pause menu
  exists to be dismissed on purpose. This also keeps the two edges asymmetric on
  purpose: focus loss is a thing the platform does _to_ the game, resuming is a
  thing the player does. _Changes it_: a sample where pausing costs the player
  something (a timed run), where the two-step would read as a penalty.

  **Read this together with "Should the click that refocuses a canvas reach the
  game at all?" above.** "Focus does not resume" is about the focus _event_. In
  a browser the gesture that delivers it is a click inside the game, so clicking
  back in onto `RESUME` does resume — one step, not two. The decision above is
  intact; the gesture is not the same gesture on every platform.

- **Which key pauses, given that a browser reserves Escape?** Taken: **Escape
  anyway.** Neither sample's action map binds it — breakout declares arrows,
  Space and R; flappy declares Space, Up and R — and it is what a player tries
  first. In a fullscreen browser demo Escape both leaves fullscreen and pauses,
  because `requestFullscreen` reserves the key and no page can decline it. That
  is one keystroke doing two reasonable things rather than a collision worth
  designing around. _Changes it_: a sample that wants Escape for something else,
  or a pause menu with a back-navigation stack where "leave fullscreen" and
  "close the menu" would want to be separate steps.

- **Who calls `requestFullscreen` in the browser — the shell or the page?**
  Taken: **the page.** A browser grants fullscreen only from inside a
  user-gesture handler; the shim's `keydown` listener is one and a
  `requestAnimationFrame` callback is not, and the engine reads a key on the
  frame _after_ the `keydown` that carried it, by which time the gesture is
  over. Calling it from Rust would also mean the wasm module's first non-`wbg`
  import, which `web/tools/check-exports.mjs` exists to prevent. So
  `web/engine/shell.js` binds F11 itself and reports the outcome through the new
  `__crcbl_web_fullscreen` entry point, exactly as a compositor answers
  `Shell::set_mode` with a configure rather than obeying it. The cost is that
  `FULLSCREEN_KEY` is spelled in four places — three `app.rs` files and
  `shell.js` — with nothing but a comment holding them together. _Changes it_: a
  second key wanting a gesture (pointer lock is the obvious one), which would be
  the point to give the shim a small table the engine can publish rather than a
  second hard-coded key.

- **Does the sandbox get a pause too?** Taken: **yes.** It has no game, and it
  does have a cube on the fixed timestep — the one thing in it a player can see
  stop — and the samples' standing rule is that a facility switched on in one is
  switched on the same way in all of them. It costs about fifteen lines.
  _Changes it_: nothing likely; if the sandbox ever became a pure benchmark
  harness, pausing it would be noise.

- **Should `MenuSet::activate` and `MenuSet::point` return the game's own
  `MenuAction` rather than a `WidgetId`?** _No — they return the id, and each
  sample maps it._ Returning the action needs a trait
  (`fn from_id(WidgetId) -> Option<Self>`) that every game with a menu must
  implement, to save one `.and_then(MenuAction::from_id)` at two call sites in
  `app.rs` and two test helpers per sample. The id is also what the layer
  beneath actually deals in: `Menu::activate` and `Menu::point` both return
  `Option<WidgetId>`, so the set passing it through adds no translation of its
  own. _Changes it_: a consumer that threads the action through several layers,
  where the `and_then` would start appearing at call sites that have no business
  knowing about ids.

- **Should the sandbox get a `MenuKind` enum for symmetry with the other four,
  instead of keying its set by `bool`?** _No._ `MenuSet<bool>` is what its one
  menu actually is, `false` is the state with no entry, and
  `apps/sandbox/src/app.rs` already called `self.menus.show(self.paused)` — the
  `bool` was always the key. An enum would be code added to make five files
  rhyme. _Changes it_: the sandbox growing a second panel, or the loop
  extraction turning out to need one `K` across all five.

- **Where does horde's "has the offer changed?" guard live now that the
  container is the engine's?** _In a `LevelUpOffer` type in
  `apps/horde/src/menu.rs`, and horde's `Loop` gained a field for it._
  `MenuSet::replace` rebuilds unconditionally and drops the capture; deciding
  _when_ a panel is stale needs `built_from: Option<(u32, [Upgrade; 3])>`, which
  the engine cannot hold because it knows nothing about upgrades. The
  alternative was putting that field on `Loop` and inlining the comparison in
  `draw_menu`, which is the same state in a place where it could not be unit
  tested. _Changes it_: a second sample growing a rebuilt panel, at which point
  the guard is a shape and not horde's alone.

## The Pages gate failed on horde about two runs in three (fixed 2026-08-03)

`Render horde in a real browser` fails two checks, both in group C/D of
`web/tools/browser-e2e.mjs`:

```
FAIL the clock advances under its own steam — x never changed
FAIL the canvas changes between frames while the simulation runs — 1 distinct frame(s) across 16 samples
```

**Measured across four Pages runs on 2026-08-03**, with the sample code
identical in the last three (`65e470b` differs from `e190e05` only in
`.github/workflows/ci.yml`, and `658c779` only in `docs/backlog.md`):

| commit    | Pages   | horde |
| --------- | ------- | ----- |
| `cd1ea54` | success | 26/26 |
| `9515b23` | failure | 24/26 |
| `e190e05` | failure | 24/26 |
| `65e470b` | success | 26/26 |
| `658c779` | failure | 24/26 |
| `588e1a6` | failure | 24/26 |

**Diagnosed and fixed on 2026-08-03 — and the heading above is wrong, which is
why it is still here.** It _was_ the code: the harness's, not the game's. The
run's uploaded page log settled it in three lines, where every theory about
timing margins had failed to:

```
[HUD] WaitingToStart  run: 1  time: 0.0
[HUD] Playing         run: 1  time: 0.0
[HUD] WaitingToStart  run: 2  time: 0.0     ← and 100 identical lines after it
```

`run: 2` is the tell. `restart` in `apps/horde/src/game.rs` is the only thing
that sets `WaitingToStart` after boot and the only thing that bumps the counter,
so the run was started and then destroyed. The cause is that check C clicked the
**centre** of the canvas to hand the page its keyboard, and `MenuKind::Start`'s
first item — `apps/horde/src/menu.rs` — is `item(Restart, "PLAY", "SPACE")`,
laid out centred. So the click pressed `PLAY`, starting the run; the `Space` the
check sent next reached a run already `Playing`, and horde binds one edge to
both "start" and "restart". The demo then sat on the title screen of run 2 with
a clock frozen at 0.0, which is exactly what checks C and D reported.

**Group E was never evidence of anything.** The "contradiction" recorded here —
four HUD lines in four seconds against one clock value in ninety — was not a
contradiction at all. `heartbeats()` counts _any_ `[HUD]` line, and horde logs
one every sixty ticks in every state including `WaitingToStart`;
`crcbl.status()` pauses and resumes a title screen as readily as a live run.
Every check in group E passes on a game sitting on its start screen. The lesson
is the general one: a check that passes in the failure mode is not a control,
and two of them agreeing is not corroboration.

**Fixed in two places.** The gate now clicks `FOCUS_CLICK_INSET` from the corner
in group C, which is what group E has always done — its comment describes
finding this same bug against the pause menu and `RESUME`, and the fix never
reached the group above it. A new check, `the focusing click pressed no button`,
asserts the game is still waiting after that click, so a harness that goes back
to pressing something fails loudly instead of poisoning the key that follows.
And `a_focusing_click_off_every_button_leaves_the_title_screen_up` in
`apps/horde/src/app.rs` is the fast half: it pins the inset against the _title_
menu the way the existing test pins it against the paused one.

**Why only horde, and why it looked like variance.** All four demos were clicked
on their centre button. The other three start screens are idempotent — clicking
`PLAY` and then pressing `Space` launches an already-launched ball — so only
horde's centre button destroys the run. Whether it failed depended on whether
both edges landed in the same tick, where `logic.intent.restart |= …` collapses
them into one, which is why an unchanged tree passed about one run in three.

**Coverage gap this leaves.** The paused-menu inset test exists in all four
samples; the title-screen one is horde's alone, because horde is where the
consequence bites. The other three would need it only if a start screen grew a
destructive first item.

## Considered and declined

- **Adopting `crcbl_ui::hud`'s `Hud`/`HudPanel` in the four samples.** It was on
  the audit's list as "the engine feature was already bought", and it is not:
  the type does not do what any of the four HUDs needs.

  **`Label` has no colour.** Colour lives on `Style`, one per panel, so a
  panel's labels are all one colour. Every sample draws its stat line yellow,
  its state line pale blue and — breakout — its lives line green, which is three
  colours in one panel and is not expressible. That alone ends it.

  Two smaller mismatches behind it. `HudPanel` sizes itself from its content,
  where horde's backdrop width is a **measured** constant with a test putting a
  stated worst-case run through the real `FontAtlas` and requiring it to fit;
  auto-sizing throws that guard away. And `Hud::render` routes button clicks,
  which a read-only stat panel has no use for.

  **What is actually shared between the four is not the drawing.** Each has a
  private `HudStrings` that rebuilds its strings only when the numbers behind
  them change — the caching is what keeps a steady-state frame from allocating.
  But the structs differ in their fields and their cache keys, because each game
  shows different numbers: that is duplicated _shape_, not duplicated knowledge,
  and the logic under it is three lines. Extracting it would be an abstraction
  over a coincidence.

  **The finding this leaves is about the engine, not the samples**:
  `crcbl_ui::hud` has no consumer anywhere in the workspace. It is either owed a
  `color` on `Label` and an optional explicit panel size — at which point the
  samples could adopt it — or it should be deleted. Not decided here, because
  adding a field nothing uses is the speculative-machinery mistake and deleting
  a module is not a call to make inside an adoption task.

- **Building the demos' export names in `web/engine/demo.js` from the sample's
  slug.**
  `exports[\`**crcbl\_${sample}\_frame\`]`would delete the thirty-line`bind`block from each`web/demos/<name>/main.js`and is the obvious way to write it. Declined because it defeats the gate:`web/tools/check-exports.mjs`learns which exports the JS depends on by scanning for a literal`.**crcbl\_…`and fails when one is missing from the artifact. Verified both directions — with the names spelled out, renaming`\_\_crcbl_breakout_frame`to`…\_framee`in`main.js`fails the check with that symbol named; behind a template literal the scan sees nothing and a typo becomes a`TypeError`
  in somebody's browser. The per-sample file is the price of keeping the check
  able to fail.
- **Folding the demo pages' "what is actually running" prose into a partial
  too.** Its opening paragraph differs between breakout and flappy by two words
  ("high score" / "best score") and its second paragraph differs materially —
  flappy's explains the seeded course, breakout's names swept-sphere collision.
  Templating it would mean the layout carrying three prose variables, which is a
  generator, not a partial. The shared blocks are the ones that are identical
  and structural: the window, the loop's keys, and the console note.
- **Reformatting `web/tools/browser-e2e.mjs` with prettier.** It is not
  prettier-clean at the width the rest of `web/` uses — confirmed against the
  version at `HEAD`, so it predates this work — and this slice touched only a
  three-line comment in it. Reformatting a 1400-line gate file to fix a
  whitespace complaint would bury that comment in a diff nobody can review.
  Worth doing on its own, with the gate run either side of it.
- **Fixing the multi-sheet sprite bug in the shader, by adding
  `SV_StartInstanceLocation` back on.** It works, and it is one line:
  `sprites[instance + base]` with `uint base : SV_StartInstanceLocation`
  restores the `BaseInstance` that `SV_InstanceID` subtracts, giving the
  absolute index that the old `draw(0..6, batch.instances)` needed. Measured
  with slangc 2026.14: the SPIR-V comes out with the `OpIAdd` next to the
  `OpISub` and no extra capability beyond the `DrawParameters` the file already
  declares.

  Declined for two reasons. First, `slangc` **rejects that semantic for WGSL** —
  `error[E55202]: system value semantic 'sv_startinstancelocation' is not supported for the current target`
  — so the source would have to be `#if`-split per target, and there is no
  target macro to split on (probed: `__TARGET_SPIRV__`, `SLANG_SPIRV`,
  `__SPIRV__`, `__TARGET_WGSL__` are all undefined; only `__SLANG_COMPILER__`
  is), so `tools/compile-shaders.sh` would have to start passing its own `-D`
  per target. Second and worse, the WGSL half would then be correct **because
  Slang's two lowerings disagree**: `SV_InstanceID` becomes
  `InstanceIndex - BaseInstance` on SPIR-V and a bare `@builtin(instance_index)`
  on WGSL, and only the SPIR-V one matches HLSL. A Slang release that made WGSL
  consistent with the rest would silently break the browser, with nothing in
  this repository pointing at the cause. Always drawing from instance 0 depends
  on neither lowering.

- **A dynamic offset on the instance _storage_ buffer rather than a per-batch
  constant block.** The obvious shape — bind `sprites` with `dynamic: true` and
  offset it to the batch — needs the binding's declared **size** to be fixed at
  bind-group creation while `offset + size` must stay inside the buffer, so the
  size would have to be "the largest batch", which is a per-frame quantity the
  group is not rebuilt for. Batches would also have to be padded to
  `min_storage_buffer_offset_alignment` (256 on WebGPU) rather than packed at
  `INSTANCE_STRIDE`. The constants block is 80 bytes and fixed, so the same
  mechanism costs nothing there.

- **Sharing `apps/*/src/audio.rs` and the best-score file between the two
  samples directly.** The duplication is real (findings 4 and 5) and the fix is
  in the engine, not in a crate the samples share between themselves: a
  `flappy-and-breakout-utils` would be a third place for the same code to rot,
  and it would hide the evidence that `crcbl-audio` and `crcbl-store` are
  missing a layer.
- **A `visible` check inside `DebugPanel::layout`.** It was written, and it
  could not be made to fail: `add` refuses to gather while hidden and
  `set_visible` drops what was gathered, so a hidden panel has no sections and
  the emptiness check already returns `None`. A guard that no test can reach is
  a guard that reports "passed" for reasons unrelated to what it guards, so it
  was deleted and the reasoning left in its place.
- **A `DebugSection::row` taking `String`s.** It takes `fmt::Arguments` instead,
  so a module writes `row("fps", format_args!("{fps:.1}"))` and formats straight
  into a `String` the section already owns. The ugly signature buys a
  steady-state section rebuild that allocates nothing, which matters for the one
  widget whose job is not to disturb the thing it is measuring.
- **Tinting one brick sprite four ways instead of authoring four frames.** It is
  the cheaper sheet and it is what `app.rs`'s colour table used to do. Four
  frames is what lets the rows differ in their _shading_ — a lit top edge and a
  shaded bottom in each row's own hue — which a single tinted rectangle cannot
  express, and it is what a sprite sheet is for. The cost is 96 × 8 texels
  instead of 24 × 8.
- **Re-randomising flappy's course from a clock.** A restart advances the seed
  deterministically (`course_seed(seed, runs)`) instead. A clock would make the
  course unreproducible, and the sample's exit criterion is that a recorded
  script replays to the same score.
- **Authoring flappy's background bands at one texel per sprite unit.** They are
  drawn at `art::BACKGROUND_SCALE` = 2 instead. At `TEXELS_PER_UNIT` = 20 a hill
  wide enough to read as a hill is a couple of hundred texels of hand-written
  rows for a silhouette with two bumps in it; the pipe is deliberately **not**
  scaled, because its caps are measured in texels and scaling would stretch
  them. If the bands ever gain detail that the doubling makes obvious, redraw
  them rather than adding a second scale knob.
