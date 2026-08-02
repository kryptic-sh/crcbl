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

- **The browser entry point was to be written once before S2. THE DEADLINE WAS
  MISSED.** Finding 2 in that list said so in as many words — "owed before S2,
  which will otherwise write it a third time" — and S2 wrote it a third time.
  `apps/breakout/src/web.rs`, `apps/flappy/src/web.rs` and
  `apps/asteroids/src/web.rs` are now the same file with three different symbol
  prefixes.

  **What it costs now, which is the part that changed.** The fix used to be
  "write it once, adopt it in two places". It is now one new shared
  implementation plus **three** call sites to migrate, three sets of `STATUS_*`
  constants to delete, three prefixes to thread through the macro, and three
  browser gates (`CRCBL_WEB_E2E_DEMO=…`) to re-run before it can be believed.
  Every sample after this adds one more of each. The three copies have barely
  drifted yet: flappy's and asteroids' were diffed with the sample name
  substituted out, and the executable difference is one extra field in the
  `finish` log line. The rest is doc-comment wording, including five link fixes
  asteroids' copy needed and the other two still need — see the rustdoc gap
  below. That is the one piece of good news and it will not survive the next
  divergent edit: `apps/*/src/audio.rs` and `apps/*/src/{best,high_score}.rs`
  are the same duplication one generation older and have already diverged in
  their public API, their type names and their file names.

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

  Why S2 did not do it: it is an engine-API change to `crates/` plus edits to
  two samples this slice was not otherwise touching, landing in the same commit
  as asteroids' audio, save file and demo page. The JS half was done as its own
  piece of work for that reason and the Rust half should be too. **It is now
  owed before S3 (horde), on the same terms and with the same warning.**

  **Status after horde 18a: still owed, and not yet violated.** That sub-slice
  is the simulation and a native window — there is no `apps/horde/src/web.rs`,
  so there is no fourth copy. `apps/horde/Cargo.toml` and `src/lib.rs` are
  already shaped for one (`crate-type = ["cdylib", "rlib"]`, a lib the bin
  links) so that adopting a shared implementation is adding a module rather than
  restructuring the package. The deadline is now the horde **web** sub-slice,
  which is the last point at which the count is three.

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
had the same set and was fixed as it was written, so it passes; the other two
are untouched because this slice's write scope did not include them.

The fix is one line in `.github/workflows/ci.yml` — a second `cargo doc` step
with `--target wasm32-unknown-unknown` over the three sample crates — plus the
nine link fixes it would then demand. Not done here because adding a required CI
job that fails on two crates this slice may not edit would land the tree red.

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

- **The bake half of `build.rs` is written four times, and it got worse.**
  `apps/flappy/build.rs`, `apps/breakout/build.rs`,
  `crates/crcbl-render/build.rs` and now `apps/asteroids/build.rs` differ in
  their `ASSETS` array and in nothing else: the same parse → bake → write →
  generate-a-table loop, the same `ART_TICK_HZ`, the same `cargo::error`
  reporting. `docs/plan/ROADMAP.md` says this was owed **before the third
  sample**; the third sample shipped with a copy instead, because closing it is
  a change to `crcbl-sprite` and to three other build scripts and the slice that
  would have paid for it was the art slice. The fix is unchanged: a real entry
  point in `crcbl-sprite` — something like
  `bake::bake_dir(manifest_dir, out_dir, &stems, tick_hz)` returning the table
  text — because a build script can depend on a workspace library and that is
  the only shape that removes the copy rather than moving it.

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
- **The panel's own cost is unmeasured.** It is designed not to perturb what it
  measures — `DebugPanel::add` returns immediately while hidden so no module's
  `debug_section` runs and no string is built, and section/row allocations are
  reused across frames — but no benchmark says what it costs when visible.
  `07-ui-debug.md`'s exit criterion is "<0.5 ms GPU for the debug overlay at
  1080p", and that number has never been taken. The CPU side is one `DrawList`
  text command per row (two allocations each, because `DrawList::text` takes an
  owned `String`) plus one background rect.
- **The overlay starts hidden in a release wasm build.** The default is
  `cfg!(debug_assertions)`, which is sample rule 4's "on by default in dev
  builds" taken literally; the demos on `crcbl.kryptic.sh` are release builds,
  so a visitor has to press F3. Whether the published demos should default it on
  is a product decision nobody has made. `web.rs` builds `Options::default()`,
  so turning it on there is one field.

## Coverage gaps

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
- **The menus' `MenuKind`/`Menus`/`MenuAction` scaffolding is written four
  times.** `apps/breakout/src/menu.rs`, `apps/flappy/src/menu.rs`,
  `apps/sandbox/src/menu.rs` and now `apps/asteroids/src/menu.rs` share the
  container, the show/select/press/activate surface and the pointer split, and
  differ in the menus they hold and what the actions do. It is the same shape as
  the `web.rs` duplication in the first section and has the same answer: the
  generic half belongs in the engine, and the per-game half — which menu belongs
  to which state, and what a button does — genuinely does not. The fourth sample
  has now arrived, so this is due with `web.rs` rather than after it.

- **The loop's pause / fullscreen / focus / pointer-capture block is written
  three times.** `apps/breakout/src/app.rs`, `apps/flappy/src/app.rs` and now
  `apps/asteroids/src/app.rs` carry the same `Loop::paused` field, the same
  `lose_focus` (drain the held keys, then pause), the same F11
  `toggle_fullscreen` reading the mode back rather than remembering it, the same
  "drain the accumulator while paused" tick loop, and the same `pointer_held` /
  `pointer_down` press-capture bookkeeping in the pump. Roughly 150 lines each.
  The three copies have not yet drifted, which is exactly when to fold them —
  the shape is a `SampleLoop` helper owning the flags and the pump's non-game
  branches, with the sample supplying its own key bindings and its own
  `MenuAction` handler. It belongs in the same slice as `web.rs` and `menu.rs`.

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
  evidence that the _shape_ is right, which the roadmap could not know.

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

Slice 18a is the core loop only. `docs/plan/sample/03-horde.md` carries the
sub-slice split and the provisional scale numbers; this is what was raised and
not finished.

- **The plan's 10 000-enemy exit criterion is not met and was not attempted.**
  `DEFAULT_MAX_ENEMIES` is 1 500, and `--max-enemies` exists so raising it needs
  no rebuild. The one measurement taken says the _simulation_ carries roughly
  8–9k at 60 Hz on the reference machine and misses 10k by about 10%; the
  **render** side of the criterion is untouched, because what draws the field
  today is one `DrawList` quad per visible enemy through the UI pass' per-frame
  vertex upload. Both halves belong to the scale sub-slice.

- **`app::MAX_DRAWN_ENEMIES` is a cap on the picture, and it is 2 000.** With
  the view cull in front of it a crowd has to be entirely on screen to reach it,
  but a crowd that does is silently truncated with no indication in the HUD. It
  exists so a frame rate measured against the placeholder renderer is not
  mistaken for a measurement of the simulation. The art sub-slice moves the
  field to `SpriteRenderer`, at which point the right answer is probably no cap
  at all.

- **Nothing enforces that the arena is a plane.** Positions are `DVec3`,
  everything the game produces sits at `z = 0` (`spawn_offset` and the seek and
  separation vectors are all planar), and `clamp_to_arena` passes `z` through
  untouched. A body given a non-zero `z` would separate in depth and never be
  brought back — which a test fixture using `DVec3::splat` did, and which is how
  this was noticed. Either clamp `z` too or make the fact a type. Not a live
  bug: no production path can produce one.

- **The horde does not avoid the walls, it is pushed into them.** Seek is a
  straight line to the player and separation knows nothing about the arena, so a
  crowd chasing a player along an edge piles into it and the clamp holds it
  there. It reads acceptably and it is not pathfinding — which is a hard
  non-goal — but "walk around the obstacle" is the first thing a player will
  expect if props ever land in the arena.

- **Contact damage has no invulnerability frames**, by choice: it is a damage
  _rate_ summed over whatever is touching, so a stack of enemies is worse than
  one and there is no per-enemy timer on the hot path. The consequence is that
  there is no way to survive being surrounded for even a moment, which is a
  difficulty decision nobody has played against yet.

- **The spawn ring is relative to the player and clamped into the arena**, so a
  player standing in a corner gets enemies materialising on the wall beside them
  instead of walking on from off screen. Rejecting and re-drawing the angle
  would fix it and would no longer be a pure function of the index, which is the
  property the determinism suite rests on; the honest fix is to pick the arc
  that is inside the arena rather than to retry.

- **Not measured, not reviewed:** the windowed native path is compiled and never
  run (no display in the build environment), so the follow camera, the HUD
  layout and the death scrim have been checked by test and by argument and by
  nobody's eyes. There is no golden image and no browser build. Frame timings
  from a headless run are the _pass_ timings only — 0.021 ms for two passes at
  an empty field, which measures nothing.

## What asteroids itself still owes

S2 is done — simulation, art, audio, persistence and the browser demo. What is
not:

- **No thrust flame still, and the audio slice was where it was going to land.**
  The previous entry here said "do it with the audio slice, where the thrust cue
  lands anyway", and the audio slice did not: `RenderState` still carries no
  thrust intent, so the ship draws one frame whatever it is doing and a player
  hears the engine without seeing it. It is two rows of `assets/ship.crpix`, a
  `bool` on `RenderState` set from `Intent::thrust`, and a frame index in
  `art::Scene::build`. Left out because the cue and the picture are different
  work and the slice was already carrying three things; the cue timer
  (`game::THRUST_CUE_PERIOD`) is the natural place to drive the flame's frame
  from when someone does it.
- **The engine cue is a one-shot on a timer, and the timer is in the
  simulation.** `crcbl-audio` has no looping voice, so a held sound is faked as
  a pulse — see `game::THRUST_CUE_PERIOD` and `crate::audio`'s header. The
  consequence is that an audio implementation detail is now deterministic tick
  state: change the period and every replay changes, even though nothing audible
  feeds back into the simulation. It is the strongest form of S1B finding 5
  anyone has produced, and it is what a real looping-voice API in `crcbl-audio`
  would delete outright.
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

## Considered and declined

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
