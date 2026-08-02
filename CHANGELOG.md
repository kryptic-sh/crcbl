# Changelog

All notable changes to this workspace are recorded here, in
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) form. Versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html); below 1.0 a breaking
change bumps the minor.

Started partway through the project, so it covers changes from this point on
rather than the whole history — `git log` is the record before it. There are no
tags yet, so everything so far is unreleased.

Internal churn a release note would not mention — refactors with no outward
effect, test-only and docs-only changes, CI repairs — is deliberately left out.

## [Unreleased]

### Added

- **crcbl-ui**, **crcbl-render**, **breakout**, **flappy**, **sandbox**: the
  samples' start, pause and end-of-game states are **menus** — a nine-sliced
  pixel-art window frame with skinned buttons inside it, centred in the
  framebuffer at every aspect ratio, replacing the flat rectangle and three
  lines of text each sample drew from its own `draw_pause_menu`.

  The art is **shared** and lives in `crates/crcbl-render/assets/menu.crpix`,
  baked by that crate's new `build.rs`: `apps/*` cannot depend on each other, so
  per-sample art would have been the same window authored three times and three
  games that looked like three engines. `crcbl_ui::menu` owns the model and the
  layout — `Menu`, `MenuItem`, `MenuStyle`, `MenuLayout`, all in screen pixels
  with no device in the room — and `crcbl_render::menu` owns the pictures:
  `MenuArt` cuts the five frames out of the sheet, `MenuRenderer` draws them
  through a `SpriteRenderer` of its own with a screen-space camera, and the
  labels stay on the UI pass. `crcbl_render::ButtonSkin` and
  `crcbl_ui::Button::with_skin`, which shipped unused, are what the buttons are
  drawn with.

  **The keyboard still works, and the mouse now does too.** Every key a sample
  bound still does exactly what it did, and each is printed on the button beside
  it; the menus add Up, Down and Enter, taken only while a menu is on screen.
  Pointer motion and clicks reach `Menu::point` through `UiState`'s press
  capture, so a press that starts on one button and is released over another
  fires neither. Both devices produce the same action.

  Behind the menu the game keeps drawing and is dimmed by a scrim sprite — drawn
  by the menu's own pass, between the game and the UI, so the panel and its
  labels are not dimmed with it. Breakout's start menu is a fresh game only:
  `WaitingForLaunch` is also where a player waits after losing a life, and a
  modal between every life would be three panels a game.

- **breakout**, **flappy**, **sandbox**: a pause state, entered and left with
  **Escape** and entered by losing window focus. A paused loop stops calling the
  game's tick, so the simulation does not advance at all; the HUD's status line
  reads `PAUSED` rather than whatever the server last thought, and a menu is
  drawn over the frame — text through the existing HUD path, behind a single
  `draw_pause_menu(&mut DrawList, extent)` per sample that the art slice
  replaces without touching the state machine. Pause is the loop's, not
  `GameState`'s: it is the loop declining to advance the simulation, and a
  `Paused` variant would put a value in the authoritative server's state that
  depends on which window a compositor has focused. `Loop::is_paused` and
  `Summary::paused` report it.
- **breakout**, **flappy**, **sandbox**: a fullscreen toggle on **F11**, which
  asks the shell for `DisplayMode::Borderless` and reads back what the window
  system actually did. There is no remembered `fullscreen` flag to disagree with
  the compositor — `Loop::display_mode` and `Summary::mode` are the _effective_
  mode, the toggle picks its target from it, and a request the window system
  refuses is logged once and reported as the mode the window really has.
- **crcbl-shell**: `__crcbl_web_fullscreen(canvas, state)`, the web backend's
  new shim entry point. A browser grants `requestFullscreen` only from inside a
  user-gesture handler and wasm is never inside one, so the page's shim makes
  the call from its own `keydown` and reports the outcome here; the backend
  moves `WindowConfiguration::mode` to match, which is what finally lets
  `WindowState::mode_request_honoured` answer `true` in a browser. An exit
  nobody asked for — Escape, which reaches no key handler — is reported the same
  way.
- **web**: `engine/shell.js` handles **F11** itself (and swallows the browser's
  own, which fullscreens the window rather than the canvas), listens for
  `fullscreenchange`, and synthesizes a focus loss on `visibilitychange` — a tab
  switch does not always blur the focused element, so `blur` alone leaves a game
  holding keys it will never see released. The demo pages gained a
  `STATUS_PAUSED` (6) status line, and `tools/browser-e2e.mjs` gained a
  focus/pause group that blurs the canvas in a real browser, checks that the HUD
  heartbeat stops, that focus coming back does not resume on its own, and that
  Escape does.

  **On a canvas, the click that restores focus is also a click in the game.**
  There is no title bar to click, so `shell.js` calls `canvas.focus()` from its
  own `pointerdown` handler — which means "clicking back in" lands a real press
  at a real position, and a press that lands on the pause menu's `RESUME` button
  resumes, exactly as it would with the game already focused. Focus itself still
  never resumes, on any platform. The two are separate and the samples' new
  `a_focusing_click_off_every_button_leaves_the_game_paused` pins them apart.

- **crcbl-ui**: `crcbl_ui::debug` — the modular debug overlay every sample now
  ships. `DebugPanel` holds `DebugSection`s and names no system; a system
  contributes by implementing `DebugModule`, whose one method fills a section it
  is handed, and the frame calls `DebugPanel::add` once per system it actually
  has. `FrameStats` is the module every frame has: a rolling window of frame
  intervals reporting FPS, average, last, best and worst. FPS is frames divided
  by the time they took, not the mean of the instantaneous rates — the two
  disagree in exactly the case a profiler exists for. `DebugOverlay` bundles the
  panel with the frame window so a sample switches the whole thing on in one
  line. `Anchor::position` is the panel's anchoring arithmetic, lifted off
  `HudPanel` so there is one copy of it.
- **crcbl-render**: `FrameTimings` implements `crcbl_ui::debug::DebugModule`, so
  the per-pass GPU timestamps that already existed appear in the overlay as a
  `gpu` section — one row per pass, plus the total and the frame number. The
  adapter lives here rather than in `crcbl-ui` because the overlay is not
  allowed to know that a render pass exists.
- **breakout**, **flappy**, **sandbox**: the debug overlay, toggled with **F3**
  and defaulting to visible in a debug build. `--debug-overlay` and
  `--no-debug-overlay` override the default. Neither game has a network module —
  both run over `InMemoryTransport` — which is what makes them the check that
  the panel composes rather than hard-codes its sections. The sandbox gained a
  UI pass to carry it; it still has no HUD and is not getting one.
- **flappy**: a second game, playable natively and at
  `https://crcbl.kryptic.sh/demos/flappy/`. One button, a bird under gravity,
  and an endless procession of pipes whose gaps are a pure function of a seed
  and the pipe's index — so the client and the server agree about the course
  without a byte of it crossing between them. It exists to find out whether the
  engine could host a game that was not breakout; what it found is written down
  in `docs/plan/ROADMAP.md`.
- **crcbl-hal**: `Device::take_error`, for the failures a backend learns about
  outside the call that caused them. Defaults to `None`, so a backend that
  reports everything through its return values is unaffected.
- **breakout**: the ball's speed ramps 2% per brick broken, capped at 1.6x the
  launch speed. A lost life and a restart both put it back.
- **crcbl-render**: `texture::upload_texture` and `UploadedTexture`, a
  format-agnostic staging upload. It replaces `ui_pass`'s private R8-only
  helper, whose row pitch was computed in texels and passed to a copy that wants
  bytes — correct only because `R8Unorm` is one byte per texel. The pitch is now
  computed in bytes and converted back once, at the copy, so an RGBA8 upload
  lands where it says it does.
- **crcbl-sprite**: a `load` feature — `decode_png`, `read_aseprite_json` and
  `load`, which take a baked sheet back apart into a `Sheet` and tightly packed
  RGBA8. §7 of `docs/specs/crcbl/pix.md` specified what the sidecar contains and
  nothing read it, so a baked sidecar was write-only. `SampleMode` does not
  survive the trip — Aseprite's schema has nowhere to put it — and that is
  asserted rather than assumed.
- **crcbl-render**: `SpriteRenderer` and `sprite.slang`, an instanced
  world-space pass that draws one quad per sprite out of a registered sheet,
  alpha blended, batched by sheet in submission order. This is the instance path
  S1B finding 1 asks for: `ForwardRenderer` draws exactly one instance, which is
  why both samples push their worlds through the UI pass. Constants go through a
  uniform buffer on every tier, so unlike `ui.slang` there is no second source
  file to keep in step.
- **crcbl-render**: `SampleMode::Pixel` is sharp bilinear, not nearest. The
  linear blend is squeezed into a band one fragment wide at each texel boundary,
  so art pixels stay flat inside and cross over in one screen pixel at any
  scale, and the sprite's screen rect is snapped to whole device pixels.
  Nearest-neighbour was the placeholder: at a non-integer scale it makes some
  art pixels four screen pixels across and their neighbours five, and the
  unevenness crawls as the sprite moves. `SpriteInstance` grew a fourth `float4`
  carrying the sheet's size and the mode, so its layout changed.
- **crcbl-sprite**: `Playback`, which advances a clip over ticks — a bare `u64`
  cursor answering `frame_index` and `finished` as a closed form, so catching up
  after a stall lands exactly where tick-by-tick would. Ping-pong shows each end
  once (period `2n - 2` looping, `2n - 1` for a one-shot that has to walk home),
  and reverse carries each frame's hold with the frame rather than reversing the
  holds too. Also `Sheet::uv`, the frame rect as normalised UVs, which every
  caller was spelling out by hand.
- **crcbl-render**: `NineSliceSource::expand`, which turns stored insets into
  the quads that draw them — corners at their natural size, edges stretched on
  one axis, centre on both. Empty bands emit nothing, so a three-slice is three
  quads and a frame with no insets is one; the cut lines are computed once and
  indexed, so adjacent quads share their edges exactly and no seam opens up. A
  target below the corners' combined size shrinks them proportionally rather
  than letting them overlap and mirror.
- **crcbl-render**: `LayerStack`, `Layer` and `Parallax` — sprites grouped into
  back-to-front bands, each taking a chosen fraction of the camera's motion. A
  layer is a container rather than a field on `Sprite`, so nothing sorts and
  submission order inside a layer is still exactly what the caller gave.
- **crcbl-ui / crcbl-render**: skinned buttons. `Button::with_skin` takes the
  nine-slice insets its art was cut with, so its minimum size and its label's
  centring follow the frame rather than being guessed; `ButtonSkin` turns a
  state and a rectangle into the quads that draw it. Resizing moves the edges
  and leaves the corners alone, which is the whole point. The skin goes through
  the sprite pass rather than the UI pass — the UI atlas is a single-channel
  glyph mask, and `crcbl-render` already depends on `crcbl-ui`, so the reverse
  could never have happened.
- **crcbl-cli**: `crcbl crpix`, which turns PNG frames into one `.crpix` sheet
  in the order given, with `--nine`, `--sample`, `--clip` and `--hold`. Frames
  are named after their file stems; two inputs whose stems collide, or a stem
  the format cannot spell back, are refused rather than written out. An existing
  output is left alone without `--force`.

### Changed

- **flappy**: the game has art. A bird with a three-frame flap, a three-sliced
  pipe, and hills and a ground band on parallax layers, all authored as `.crpix`
  text under `apps/flappy/assets/` and baked to PNG + sidecar by a new
  `build.rs` — nothing baked is committed, so the text is the only source of
  truth and editing it rebuilds the game. The pipes were screen-space UI quads
  and the bird a lit cube through the forward pass; both are sprites in world
  coordinates now, drawn by `SpriteRenderer` between a `sky` clear and the HUD.
  Nothing about how the game _plays_ changed.
- **flappy**: `ForwardRenderer` is gone from the frame, and with it the HDR
  scene target, the depth buffer, the tonemap pass and the cube. The forward
  pass drew exactly one instance and the bird was it; a one-line `clear_color`
  pass replaces the clear it also happened to do.
- **breakout**: the board is art. Four bevelled brick frames — a brick's frame
  is read back out of its row, so a row's colour follows its position rather
  than being tracked beside it — a paddle, a ball, and a nine-sliced stone court
  whose wall faces land exactly on the colliders the ball bounces off. Authored
  as `.crpix` under `apps/breakout/assets/`, baked by a `build.rs` like
  flappy's. The forty bricks went through the UI draw list and the paddle was
  the one lit mesh; both are sprites now, and `ForwardRenderer` is gone from
  breakout too.
- **flappy**: the wing beats when the player flaps. The clip was a free-running
  loop that never looked at the bird, so the animation and the button had
  nothing to do with each other; a rising vertical velocity is exactly a flap,
  and it restarts the clip.
- **demo site**: the demo window is **one template**. The terminal frame, the
  canvas, the status bar, the focus note, the three keys the engine's loop keeps
  and the console note were the same markup written out per demo page; they are
  `web/templates/demo-*.html` now, pulled into a page with `<!--include …-->`.
  `build-pages.py` fails the build for a demo page that does not include them,
  so the next demo cannot go back to a copy.
- **demo site**: `web/engine/demo.js` is the boot sequence and the frame loop
  for every demo. `web/demos/breakout/main.js` and `web/demos/flappy/main.js`
  were 288 lines each and differed in the sample name, one status line and one
  comment — the shape that had already shipped breakout's control hint on
  flappy's page. Each is ~30 lines now: this sample's `__crcbl_<name>_*`
  symbols, written out literally so `check-exports.mjs` still sees every one,
  plus what to press and what it saves.
- **web tooling**: `check-exports.mjs` and `smoke.mjs` take `--sample <name>`,
  and `run-browser-e2e.sh` takes `CRCBL_WEB_E2E_DEMO`. Each was written when
  there was one demo and asserted against the whole workspace or against
  breakout's own strings, so the second demo broke all three. A sample's
  contract is now scoped to that sample, and the browser gate refuses a demo it
  has no expectations for rather than passing on a game that never started.

### Fixed

- **crcbl-render**, **crcbl-shaders**: the sprite pass drew **every batch after
  the first from the first batch's sprites** on Vulkan. A batch is a run of
  sprites sharing a sheet, and `SpriteRenderer::add_pass` pointed each draw at
  its slice of the frame's instance buffer with `firstInstance` — but `slangc`
  lowers `SV_InstanceID` to `InstanceIndex - BaseInstance` for SPIR-V, so the
  index restarted at zero for every batch and each one redrew the first batch's
  sprites with a later sheet bound. A four-sheet frame put one rectangle on
  screen and left the rest empty. **Both samples register four sheets**, so
  `breakout` and `flappy` were affected on every native run since the pass
  shipped; the browser was not, because `slangc` lowers the same source to
  WGSL's `@builtin(instance_index)`, which WebGPU defines to include
  `firstInstance`.

  No shader source is correct on both targets while `firstInstance` is non-zero,
  so it is now always zero: every draw is `draw(0..6, 0..count)` and the batch's
  offset arrives in the new `SpriteConstants::base` field, through a
  dynamic-offset binding of set 0. **`SpriteConstants` is one block per batch
  rather than one per frame**, laid out at `SpriteRenderer::constant_stride()` —
  `CONSTANTS_SIZE` rounded up to the device's
  `min_uniform_buffer_offset_alignment` — and its `pad: [f32; 2]` has become
  `base: u32, pad: u32`. Callers of the pass are unaffected; anyone building
  `SpriteConstants` by hand is not.

  `crates/crcbl-vk/tests/vk_e2e.rs` gains a golden of three solid-colour sheets
  at four rectangles, which is red against the old pass; the batching tests in
  `crcbl-render` now pin the draw ranges at zero and the dynamic offset per
  batch.

- **breakout**, **flappy**: a window that lost focus kept playing, and kept
  saying so. The samples ignored `ShellEvent::Focus` entirely — on every
  platform, native and browser — so alt-tabbing away left the simulation running
  with the HUD reading `Playing`, and a life was lost while nobody was looking.
  Focus loss now pauses the loop and releases every key the game thinks is held,
  which is the obligation `ShellEvent::Focus`'s own documentation states: no
  platform delivers releases for keys held when focus leaves. Flappy had the
  worse half of it — its flap is an edge, and an action map that never saw Space
  come up raises no further `just_pressed`, so the bird could never flap again.
  Regaining focus deliberately does not resume.

- **crcbl-wgpu**: a shader module or pipeline that fails to build is reported.
  WebGPU hands back an object either way and delivers the reason to the device's
  error channel, so failures were invisible: the backend built a pipeline on a
  module that had not compiled and every submission after it was silently
  discarded, which presents as a black canvas over a game that reports itself as
  playing. Creation calls now return `HalError::Backend`, and the asynchronous
  half — the browser's, which no call can be blamed for — stops the frame loop
  from `GpuContext::acquire` with the driver's own message.
- **breakout**: the ball is no longer under gravity. It launches at a constant
  speed and collisions change only its direction, which is what makes a shot
  aimable.
- **breakout**: the paddle steers, by being moved. A paddle standing still
  mirrors the ball like a wall; a paddle being driven left or right decides
  which way the ball goes next, and turns a ball back the way it came rather
  than rebounding it onward.
- **breakout**: the whole play field is on screen at every aspect ratio. The
  orthographic camera derived its width from a fixed half height, so a 4:3
  surface — the size the window opens at, and the aspect the web demo's canvas
  is styled with — cropped two world units from each side and the ball
  disappeared off the edge before bouncing back.
- **crcbl-phys**: `PhysicsWorld::sweep_sphere` reports contacts it used to miss.
  The broadphase traversed the sphere's centre line, so anything the sphere
  overlapped by less than its radius was dropped before the exact test, and a
  contact landed only once the centre reached the surface.
- **crcbl-store**: `canonical_key` and the browser backends split keys on `/` on
  every platform. Parsing went through `std::path::Path`, whose separators are
  the host's, so `a\b` was refused on Linux and quietly rewritten to `a/b` on
  Windows.

[Unreleased]: https://github.com/kryptic-sh/crcbl/commits/main
