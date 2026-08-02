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

- **The browser entry point should be written once, before S2.** Finding 2 in
  that list. `apps/flappy/src/web.rs` and `apps/breakout/src/web.rs` are the
  same file with a different symbol prefix, and asteroids will be the third.
  What it would take: a crate (or a `crcbl` module) owning the `Stage` state
  machine, the log queue and the `prepare`/`boot`/`frame`/`status`/`shutdown`
  protocol, with the sample supplying only its prefix and its two loop types.
  The prefix has to stay per-sample — two demos can be open in one browser and
  the symbols must not collide — so the shape is probably a macro over a generic
  core rather than a plain function. **The JS half of this is now done** —
  `web/engine/demo.js` is one boot sequence for every demo and
  `web/demos/<name>/main.js` is only the prefix — and it settles the shape
  question in the affirmative: the sample-specific part turned out to be ten
  literal symbol names and two strings, and nothing else. The Rust half stands.

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
`a_focusing_click_off_every_button_leaves_the_game_paused` in both samples'
`app.rs` asserts the corner is over no button and the centre is over `RESUME`,
so a menu that grew until it reached the corner fails a fast Rust test rather
than the slow browser one.

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

- **The bake half of `build.rs` is written three times.**
  `apps/flappy/build.rs`, `apps/breakout/build.rs` and now
  `crates/crcbl-render/build.rs` differ in their `ASSETS` array and in nothing
  else: the same parse → bake → write → generate-a-table loop, the same
  `ART_TICK_HZ`, the same `cargo::error` reporting. Asteroids will be the third.
  The fix is a real entry point in `crcbl-sprite` — something like
  `bake::bake_dir(manifest_dir, out_dir, &stems, tick_hz)` returning the table
  text — because a build script can depend on a workspace library and that is
  the only shape that removes the copy rather than moving it.

- **The tick rate the art is baked at is written twice per game** —
  `ART_TICK_HZ` in `apps/*/build.rs` and again in `apps/*/src/art.rs`. A build
  script cannot `use` the crate it builds, and the sidecar's durations are
  milliseconds, so the two conversions have to agree. Guarded rather than
  solved: each game's `the_art_bakes_to_the_sheets_it_declares` asserts an
  authored hold in ticks survives the round trip. **Breakout's guard is weaker
  than flappy's**, because nothing breakout draws is animated: it can only
  assert the default hold of 1 tick, which survives a fairly wide range of wrong
  rates. It gets real the moment breakout has a clip. Folding it into the
  `bake_dir` entry point above would close it outright.

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

- **Nothing in the workspace sets a non-zero rotation yet.** Both samples pass
  `rotation: 0.0` at all five of their `Sprite` literals, and there is no
  interpolation of an angle anywhere: `crcbl-phys` has no angular velocity (see
  "What asteroids still needs from `crcbl-phys`" below), and the interpolation
  buffer carries `Transform`s, whose `DQuat` no sprite path reads. Asteroids is
  the first caller and will need to decide where the ship's angle comes from —
  game code writing it through `set_transform` each tick, then a `slerp` or a
  scalar lerp in the render path — because a turn rate applied to the _rendered_
  angle without interpolation will stutter at any frame rate that is not the
  tick rate. That is the next real question this slice does not answer.

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
- **The menus' `MenuKind`/`Menus`/`MenuAction` scaffolding is written three
  times.** `apps/breakout/src/menu.rs`, `apps/flappy/src/menu.rs` and
  `apps/sandbox/src/menu.rs` share the container, the show/select/press/activate
  surface and the pointer split, and differ in the menus they hold and what the
  actions do. It is the same shape as the `web.rs` duplication in the first
  section and has the same answer: the generic half belongs in the engine, and
  the per-game half — which menu belongs to which state, and what a button does
  — genuinely does not. Worth folding in when the fourth sample arrives, with
  `web.rs`.

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

## What asteroids still needs from `crcbl-phys`

P6 delivered dynamic BVH churn, sphere overlap against the broadphase, and the
first two L1 force providers. What the sample doc
(`docs/plan/sample/02-asteroids.md`) names and this slice did **not** deliver:

- **Segment CCD as a single entry point.** `PhysicsSystem::sweep_sphere` exists
  and is what a bullet should go through (`prev → cur` as the segment, the
  bullet's radius as the sweep radius), so the machinery is there. What is not
  there is a bullet-shaped API — the sample will be writing "sweep from where it
  was to where it is, and if it hit anything, that is the impact" by hand, in
  every sample that fires anything. Decide when asteroids writes it whether it
  earns a named method. Not blocking.

- **`PhysicsSystem::overlap_sphere` fabricates its `ShapeHit`.** It returns
  `t: 0.0`, `normal: DVec3::Y`, `started_inside: true` for every result — those
  are not measurements, they are filler. Asteroids only asks _whether_ the ship
  touched a rock, so it does not bite yet, but the type promises a contact and
  does not deliver one. Either compute a real deepest-point normal or change the
  return type to entity ids. `PhysicsWorld::overlap_sphere` underneath it is
  honest — it returns `Vec<ColliderId>` and nothing more.

- **Rotational dynamics are absent.** `Transform` carries a `DQuat` and
  `ThrustForce` reads it, but there is no angular velocity, no torque and no
  quaternion integration: `RigidBody` has `velocity` and `force_accum` and
  nothing angular. A ship that turns must have its rotation written by game code
  through `set_transform`. That is fine for asteroids (turn rate is a constant,
  not a physical response) and wrong for the inertia tensor the design doc
  describes. Whoever needs real torque adds `angular_velocity`, `torque_accum`
  and an inertia term to `RigidBody` and a rotation step to `SemiImplicitEuler`.

- **Screen wrap has no broadphase story.** The sample doc says the wrap teleport
  "exercises `WorldPos` rebase + broadphase re-insertion". A teleport today is a
  `set_transform`, which refits the leaf where it stands — and
  `Bvh::update_aabb` deliberately does not re-pick the leaf's place in the tree,
  so a body that jumps across the playfield leaves its ancestors' bounds
  stretched across the whole field. Remove-and-re-insert is the correct move for
  a teleport and nothing does it automatically. Cheap to add (`PhysicsWorld`
  knows both), but it needs a rule for _when_ — a distance threshold, or an
  explicit `teleport()` call by the caller. Left for whoever writes the wrap.

- **No benchmark, and no rebuild policy.** Churn cost was measured as tree
  _depth_, not as time: the claim "insert/remove beats a rebuild" is an
  algorithmic one (one root-to-leaf path against `O(n log n)`) and was not
  timed. There is also no policy that ever rebuilds a churned tree — the AVL
  bound is what makes one unnecessary, but a bulk build still produces a tighter
  tree by surface area than incremental insertion does, and nobody has measured
  the query-cost difference between the two. The horde sample (P8, 10k bodies)
  is where that stops being academic.

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
  waiting.

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
