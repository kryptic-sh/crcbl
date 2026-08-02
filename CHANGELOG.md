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
- **web tooling**: `check-exports.mjs` and `smoke.mjs` take `--sample <name>`,
  and `run-browser-e2e.sh` takes `CRCBL_WEB_E2E_DEMO`. Each was written when
  there was one demo and asserted against the whole workspace or against
  breakout's own strings, so the second demo broke all three. A sample's
  contract is now scoped to that sample, and the browser gate refuses a demo it
  has no expectations for rather than passing on a game that never started.

### Fixed

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
