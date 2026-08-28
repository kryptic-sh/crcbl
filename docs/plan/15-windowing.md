# Topic 15 — Windowing (`crcbl-shell`)

From-scratch windowing, replacing winit. Own event loop, windows, monitors,
input, and surface handles behind one trait; native backends per platform, with
**all windowing logic ours** — we own the code and the bugs above the OS/driver
ABI. Godot's `DisplayServer` is the closest prior art for the shape; no
windowing framework underneath ours. (The original "zero third-party code"
phrasing is superseded by the policy section below — the Vulkan WSI ABI makes it
unachievable, and pretending otherwise would have blocked P1.)

## The dependency line: bindings, not frameworks (REVISED — see WSI note)

The rule is **"no framework makes decisions for us"**, not "no code but ours
links into the process":

- **Rejected**: frameworks that own policy — winit, SDL, GLFW (windowing), and
  by the same logic egui (UI) and wgpu-as-the-performance-tier.
- **Accepted**: thin bindings to APIs the OS or driver requires _by ABI_ — `ash`
  (already locked for Vulkan), `objc2`/`windows-rs` (09), and on Linux
  **libwayland-client / libxcb for the connection and proxy objects only**.
  Everything above those handles — protocol selection, event loop, window
  lifecycle, DPI, input, modes — is ours.

### Why the Linux exception is forced (the WSI ABI)

A hand-rolled wire-protocol client **cannot present through Vulkan.**
`vkCreateWaylandSurfaceKHR` takes a real `wl_display*` / `wl_surface*`, and the
driver's WSI implementation calls libwayland functions on them
(`wl_proxy_marshal_flags`, its own event-queue dispatch); the same is true of
`vkCreateXcbSurfaceKHR` and a genuine `xcb_connection_t*` (the driver issues
Present-extension traffic on it). Objects we invent from raw socket bytes are
not those objects. This was missed in the first draft of this plan and would
have blocked the P1 gate ("lit mesh on screen") on day one.

So, LOCKED for P0:

- **libwayland-client / libxcb provide the connection and proxy objects**; our
  codegen sits on top of `wl_proxy_marshal_flags` (exactly how the Rust
  wayland-client crates work — we own the protocol layer, not the transport
  ABI). `wayr` is the donor for that layer.
- **Full independence remains possible and is documented, not scheduled**:
  render offscreen, export `VkDeviceMemory` as a dma-buf
  (`VK_EXT_external_memory_dma_buf`), and present via `zwp_linux_dmabuf_v1` +
  `linux-drm-syncobj-v1` explicit sync + `wp_presentation` pacing — a real
  subsystem (and one the earlier protocol list omitted entirely, which is how
  the cost stayed hidden). Revisit only as a deliberate exercise, never as a P0
  assumption.
- Windows/macOS keep hand-written FFI where it's small, but the policy above
  means `objc2`/`windows-rs` in the HAL (09) is **not** a contradiction —
  bindings are fine; frameworks are not.

## Display modes (LOCKED — two, not three)

Exclusive/true fullscreen is **dropped** deliberately: Wayland can't modeset by
design, macOS fights it, and on Windows borderless + DXGI flip-model gets
independent-flip/MPO latency ≈ exclusive without the alt-tab disasters. Two
modes cover everything:

| Mode           | Surface                                                   | Resolution                                                                                                                      |
| -------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| **Windowed**   | decorated OS window, freeform **or aspect-locked** resize | render resolution = client area, 1:1; resize recreates swapchain                                                                |
| **Borderless** | frameless window at monitor size, desktop mode untouched  | **render scale**: internal render target at chosen resolution (e.g. 1920×1080), upscale-blit to native surface (e.g. 2560×1440) |

Render scale is a **renderer** feature (stage 2/3): offscreen target at internal
res → upscale pass to swapchain (bilinear MVP; sharpening/FSR-class filters slot
in later; UI can render at native res post-MVP for crispness). The shell only
ever presents native-size surfaces in borderless. Wayland fast path:
`wp_viewport` hands the compositor the smaller buffer and display hardware
scales — same visible result, zero shader cost; the blit remains the portable
path.

Aspect lock per backend: Windows `WM_SIZING` rect adjust; macOS
`setContentAspectRatio`; X11 `WM_NORMAL_HINTS` aspect; Wayland has no aspect
hint — pick nearest aspect-correct size in the `configure` round. **Letterboxing
in-renderer is the universal fallback** (tiling WMs and compositors can force
any size) and must always work.

## The display settings catalogue (LOCKED 2026-08-27)

The two modes above are one row of a much larger surface. Every game ships a
display menu, and a menu whose key names are invented while it is being built is
a `settings.toml` the next version cannot read — so the **whole** player-facing
display surface is named here before it is built: one key each, its value
domain, what it clamps, and what implements it today.

Three documents share this surface and the split is deliberate:

- **This one** owns the rows that are properties of a window, a surface or a
  monitor.
- [39-capabilities.md](39-capabilities.md) owns the graphics-quality rows and
  the rule that decides how a level-valued key clamps, because the four-layer
  resolution order those rows feed already lives there.
- [14-persistence.md](14-persistence.md) owns the file — the `[engine.video]`
  section, the spelling convention, and what happens when there is nowhere to
  write it.

The keys are `[engine.video]` keys and the spelling convention is **adopted, not
invented**: `crcbl_store::settings`' own module example already writes `vsync`
and `resolution = [1920, 1080]` in that section, so this catalogue follows it
rather than competing with it. Bare snake_case nouns, no negated keys — the
argument is on `crcbl::settings::VIDEO_KEYS`, which is the one place a key is
spelled today.

**Almost none of this is implemented, and the "Today" column says so per row**
rather than once at the end. **Every row below is now also in code**, as of
2026-08-28: `crcbl::settings::catalogue` carries each key with its domain and a
`KeyStatus` of `Read` or `Named`, so a settings screen and `crcbl settings list`
read the same list this table states rather than a second copy of it. The name
and the domain are fixed now anyway, which is
[14-persistence.md](14-persistence.md)'s second catalogue rule: a key named late
is a file every existing player has already written.

| Key            | Domain                                                          | Today                                                                                                                                                                                                                                                                       |
| -------------- | --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `display_mode` | `"windowed"` \| `"borderless"`                                  | **The switch is built and the persistence is not.** `DisplayMode` and `Shell::set_mode` ship on every backend, and `crcbl::engine::ModeRequest::toggle` drives them from the engine's own pause menu. No key reads or writes it.                                            |
| `monitor`      | monitor **name**; absent means "wherever the window already is" | `Shell::monitors` and `DisplayMode::Borderless { monitor }` exist. No key reads it.                                                                                                                                                                                         |
| `resolution`   | `[width, height]` in device pixels — the **surface** extent     | The shell creates a window at a requested size and `SwapchainDesc::extent` is that size. No key reads it, and nothing resizes a window from a setting.                                                                                                                      |
| `present_mode` | `"auto"` \| `"vsync"` \| `"adaptive"` \| `"off"`                | `crcbl::engine::Pacing` implements all four and `GpuContext::set_pacing` applies them to a live swapchain. No key reads it.                                                                                                                                                 |
| `frame_limit`  | frames a second; `0` is unlimited                               | **Read as of 2026-08-28.** `crcbl::settings::frame_limit` reads the key as a _ceiling_ and `FrameLimit::clamped_to` applies it, since "less" here means less than the game's own cap; `GpuContext::frame_limit` is the pair a caller uses. Nothing in `apps/` calls it yet. |
| `render_scale` | fraction of the surface extent — the **internal** extent        | **The renderer half is built and no key reads it.** `ForwardRenderer::set_render_scale` sizes the internal target and `shaders/upscale.slang` reconstructs it into the caller's; `MIN_RENDER_SCALE` is the floor. No settings key and no `Shell` request.                   |
| `brightness`   | scalar multiplier applied in the tonemap pass                   | **Nothing.** `crcbl_shaders::tonemap::TonemapParams` carries `exposure` and no second field; brightness would be that field.                                                                                                                                                |
| `hdr_output`   | `true` \| `false`                                               | **Nothing, and further from built than the rest** — see the HDR note below.                                                                                                                                                                                                 |
| `ui_scale`     | multiplier over the window's own scale factor, UI only          | **Nothing global.** `crcbl_ui::MenuStyle::pixel_art` takes a whole-number scale per menu; no crate applies a player-chosen factor to UI layout.                                                                                                                             |
| `fov`          | vertical field of view in degrees                               | `crcbl_render::Camera::Perspective`'s `fov_y` is the value a game sets in code. No key reads it.                                                                                                                                                                            |

### `resolution` and `render_scale` are two different resolutions

This is the row a settings menu gets wrong, and getting it wrong is not a
cosmetic bug: a player who asked for cheaper pixels gets a resized window
instead, on a machine that was already struggling.

- **`resolution` is the surface extent.** It sizes the OS window and therefore
  the swapchain, and the whole frame — including the UI — is drawn at it. In
  **borderless** it is not settable at all: borderless covers the monitor, which
  is the display-mode table's own definition, so a `resolution` key present
  alongside `display_mode = "borderless"` is ignored rather than obeyed.
- **`render_scale` is the internal extent.** It sizes an offscreen target that
  the scene and the post chain are drawn into, and the result is upscaled to the
  surface. Nothing about the window changes. This is what makes the frame
  cheaper: [18-render-features.md](18-render-features.md) orders the post stack
  before the upscale precisely so its cost scales with the internal extent, and
  composites the UI after it at native resolution.

Both numbers can move independently and a menu must show them as two rows. The
one thing they share is that **`render_scale` is now half-built**: the renderer
resamples for real, and nothing on the settings or shell side can ask it to —
see the correction below.

### Refresh rate is read, never written

There is no `refresh_rate` key, and the reason is the locked two-mode decision
above: **the engine never modesets.** A borderless window leaves the desktop's
video mode untouched by definition, and a windowed one has never had a say. So
refresh rate is an _observation_ — `MonitorInfo::refresh_millihertz`, with
`refresh_hz` for display — belonging to the same family as
`crcbl_hal::DisplayTiming`, which reports what the presentation engine is
actually doing rather than what was asked for. A settings screen shows it; it
does not offer it.

Writing one would mean acquiring the display, which is exclusive fullscreen
under another name, which is the thing this document dropped.

### HDR output is a swapchain negotiation, not a brightness slider

`hdr_output` gets its own key rather than being folded into `brightness`,
because the two are not the same mechanism and never become the same mechanism.

Note also that topic 18's "HDR" is **not** this: that is the internal working
space — an `Rgba16Float` offscreen target that a tonemap pass resolves into an
sRGB swapchain, and `SwapchainDesc::format`'s own doc comment says the swapchain
format is "a display format, never a shading one". HDR _output_ means presenting
in a wide colour space and letting the display do the mapping, which needs a
colour space beside the format on both `SwapchainDesc` and `SurfaceCaps` —
neither of which has one. That seam change is the first thing this key costs,
and it is a HAL change, not a shell one.

### The two rules this catalogue lives under

Stated once each, in the document that owns them, and binding on every row here:

1. **The `[engine.video]` layer may only clamp downward, and an absent key
   clamps nothing.** [39-capabilities.md](39-capabilities.md) carries the rule
   and what "downward" means for a key whose value is a level rather than a
   boolean.
2. **A key with no implementation still gets its name and its domain now.**
   [14-persistence.md](14-persistence.md) carries the rule and the list of which
   keys those are.

The display rows are where rule 1 is least intuitive, so it is worth saying what
it means here: `display_mode = "borderless"` does **not** force a game into
borderless. It is a ceiling like every other key in the section — a game that
opened windowed stays windowed, and a player who wants the switch uses the
control that already exists for it. The keys that have a genuine downward
reading are `render_scale`, whose lower values are cheaper, `resolution`, whose
smaller extents are cheaper, and `frame_limit`, whose lower rates are cheaper —
and that last one is the row that shows what "downward" costs to implement,
because its zero means _unlimited_ and therefore sits at the top of the order
rather than the bottom. `crcbl::engine::FrameLimit::clamped_to` is where that is
written down. `monitor`, `fov` and `ui_scale` have no ordering at all, which is
discussed under rule 1 rather than here.

### Considered and declined

- **A per-monitor or per-adapter settings profile.** The idea is that unplugging
  a laptop from a 4K display should restore the settings that machine last used
  at 1080p. It is declined because it makes settings **two-dimensional** — every
  key gains an implicit "under which hardware" axis, the file stops being the
  diff-against-defaults that keeps it small, and the resolution order in
  [39-capabilities.md](39-capabilities.md) grows a fifth layer whose input is a
  monitor name that `MonitorInfo::name`'s own doc comment says is neither unique
  nor stable across drivers. Nothing in this workspace has asked for it. Revisit
  only with a concrete report of a player losing settings to a hardware change,
  and revisit it as a _profile_ mechanism in
  [14-persistence.md](14-persistence.md), not as a second axis bolted onto the
  key space.
- **`monitor` as an index rather than a name.** An index is stable until a
  monitor is hotplugged, at which point every stored index means a different
  screen. The name is not unique either — `MonitorInfo::name` says so — which is
  why the key is a **hint**: resolve by name, fall back to the primary monitor,
  and never fail a start-up over it.
- **A `refresh_rate` key.** See above; it would be modesetting.
- **Exclusive fullscreen** stays dropped. The Display modes section above has
  the reason and the 2026-08-09 correction below records that `crcbl-dx12`
  enforces it from beneath the seam.

## The seam: platform-agnostic by construction

Same discipline as the HAL: **no platform type ever crosses the trait
boundary**. Consumers (engine loop, renderer, samples, editor) compile against
`crcbl-shell` types only; `#[cfg]` in a consumer is a regression.

```rust
// All POD/engine types — nothing platform-specific in any signature.
pub trait Shell {
    fn create_window(&mut self, desc: &WindowDesc) -> Result<WindowId, ShellError>;
    fn set_mode(&mut self, win: WindowId, mode: DisplayMode);      // Windowed{..}|Borderless{monitor}
    fn set_constraints(&mut self, win: WindowId, c: SizeConstraints); // min/max/aspect
    fn monitors(&self) -> &[MonitorInfo];                          // geometry, scale, refresh
    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent));          // drives tick(dt) inversion
    fn surface_target(&self, win: WindowId) -> SurfaceTarget;      // opaque — see below
    fn set_pointer_mode(&mut self, win: WindowId, mode: PointerMode); // free|locked|confined
    fn set_cursor(&mut self, win: WindowId, cursor: Option<CursorIcon>);
    fn caps(&self) -> ShellCaps;
    // clipboard, drag-drop enable, close-request reply …
}
```

- **`ShellEvent`**: engine-typed enum — `Resized`, `ScaleFactorChanged`,
  `CloseRequested`, `Focus`, `Key{scancode, keysym, state}`,
  `PointerMotion{abs, raw_delta}`, `Button`, `Wheel`, `TextCommit`,
  `MonitorsChanged`, `DroppedFile`. Timestamps preserved on input events (feeds
  the P2 input pipeline).
- **`SurfaceTarget`**: the _single_ sanctioned platform leak — an opaque handle
  only HAL backends destructure (wayland display+surface ptrs, xcb conn+window,
  HWND, NSView, canvas id). Shell consumers can't open it; the HAL backends —
  `crcbl-vk`, `crcbl-mtl`, `crcbl-dx12`, `crcbl-webgpu` — can. Mirrors
  raw-window-handle's role, but ours.
- **`ShellCaps`**: capability flags instead of platform sniffing — `hw_upscale`
  (Wayland viewport path), `aspect_hint_honored` (native aspect lock vs
  letterbox-only), `pointer_warp`, `text_ime`. The renderer picks
  blit-vs-viewport and windowed-aspect behavior from caps, never from "am I on
  Wayland".
- **Backend selection at runtime**, not compile time, on Linux: try Wayland
  socket → fall back to X11 (both compiled in; `CRCBL_SHELL=x11` override).
  Other OSes have one backend each.
- **`HeadlessShell`** is a complete first-class implementation (fixed-size
  virtual monitor, scripted event injection, no OS calls) — CI and
  `crcbl screenshot`/`sim` run the identical engine loop through it. The seam is
  proven agnostic the same way the HAL is: NullBackend compiles + runs
  everything above it.

## Scope of `crcbl-shell`

- Event loop (owned by the shell; engine driven as `tick(dt)` callbacks — the
  wasm-friendly inversion from stage 1 is unchanged).
- Window create/destroy, title, mode switch (windowed ↔ borderless on a chosen
  monitor), min/max/aspect constraints, close-request interception.
- Monitor enumeration: geometry, work area, DPI/scale, refresh rate; monitor
  hotplug events.
- DPI: one `scale_factor` concept over fractional-scale-v1 (Wayland),
  per-monitor-v2 (Windows), `backingScaleFactor` (macOS).
- Input: keyboard (scancode + layout-mapped keysym), mouse (delta + absolute,
  raw motion for gameplay, pointer lock/confine), wheel, text input events (full
  IME post-MVP; commit-string basics MVP), cursor set/hide.
- Raw surface handles for the HAL (wl_surface/xcb window/HWND/NSView/canvas).
- **Clipboard**: mime-typed get/set — `text/plain` always, plus the custom mime
  `application/x-crcbl+ron` so engine↔engine copies are lossless while outside
  apps still receive readable RON text (offer both, reader picks). Backend
  realities owned like everything else: Wayland `data-device` offers; X11
  selections protocol incl. `TARGETS` negotiation + `INCR` chunked transfers
  (the classic X11 clipboard iceberg — scoped to what we offer/accept); Win32
  `OpenClipboard`/`CF_UNICODETEXT` + registered format; macOS NSPasteboard; web
  async `navigator.clipboard` (permission-gated, paste requires user gesture —
  surfaced via `ShellCaps.clipboard` so the editor UI degrades gracefully
  in-browser). File-list transfers ride the same machinery (`text/uri-list` on
  Linux, `CF_HDROP`, NSPasteboard file URLs) — enables OS-file paste into the
  editor's asset browser later with zero seam changes.
- Drag-drop: file paths in (viewer/editor import), same mime set as clipboard —
  DnD and clipboard share the offer/receive plumbing on Wayland/X11 anyway (one
  implementation, two triggers).
  - **Status:** implemented on Wayland (P0.5c) and on Win32 (P5C W3, through
    `WM_DROPFILES` — small enough to be in scope, unlike XDND). **XDND is
    deferred on X11** — it is a five-message handshake over a _second_ selection
    with its own version and timestamp rules, i.e. the whole selection machinery
    again with a protocol on top, and it was its own slice on Wayland.
    `ShellCaps::DRAG_DROP` is honestly clear on the X11 backend and
    `DroppedFile` is never emitted there. Owed before the editor's asset browser
    needs OS-file drops (P12), and the `accept_drops` gate plus `parse_uri_list`
    already carry over unchanged.

Explicitly out (post-MVP or never): exclusive fullscreen, multi-window MVP
(editor is single-window until it isn't), gamepad raw backends
(evdev/XInput/GameController per topic 19 schedule), touch.

## Backends

| Platform | Backend                                                                                                  | Lands |
| -------- | -------------------------------------------------------------------------------------------------------- | ----- |
| Wayland  | libwayland-client connection/proxies + **our** protocol codegen on `wl_proxy_marshal_flags` (wayr donor) | P0    |
| X11      | libxcb connection + **our** request/event layer (core, EWMH atoms, RandR, XKB)                           | P0    |
| Web      | canvas + DOM events via our own minimal JS shim + wasm imports                                           | P5    |
| Windows  | hand-written Win32 FFI (`extern "system"` decls for the surface we use)                                  | P5C   |
| macOS    | hand-written Objective-C runtime FFI (`objc_msgSend`) to AppKit                                          | P5C   |

Notes on the from-scratch protocol work:

- **Wayland**: we generate marshaling from the protocol XMLs with our own
  build-time codegen, emitting `wl_proxy_marshal_flags` calls against
  libwayland-client's connection (the WSI ABI requirement above). Needed
  protocols: core, `xdg-shell`, `xdg-decoration`, `wp_viewporter`,
  `fractional-scale-v1`, `pointer-constraints` + `relative-pointer` (raw
  motion/lock), `data-device` (clipboard/DnD) — plus `zwp_linux_dmabuf_v1`
  **only** if the independent-presentation path is ever taken.
- **X11**: core protocol subset (window create/map, atoms/EWMH for fullscreen +
  hints, input events) over libxcb's connection, RandR for monitor enumeration,
  XKB for keymaps. Request/event layer is ours; scope stays at what the shell
  actually uses.
- **Windows/macOS**: FFI declarations are code we write and own — dozens of
  functions, not thousands; audited by use. They were originally scheduled with
  Metal/DX12 because "before that they'd be compile-verified-only anyway (gpur
  lesson: that's not support)", and that reasoning turned out to be the HAL's
  rather than the shell's: P0.6 tested a whole X11 backend against a real server
  with no renderer in existence, and both CI runners are desktops. A shell
  backend does not need a GPU backend to be tested against a real desktop, which
  is why these landed at P5C instead.
- **Web**: `wasm-bindgen` is avoided in the shell; a small hand-rolled JS glue
  file exports canvas/event/rAF hooks as plain wasm imports. Nothing else in a
  browser build uses it either — see [10-wasm-webgpu.md](10-wasm-webgpu.md)'s
  deviations.

## Testing (topic 12)

- Shell trait gets a `HeadlessShell` (the windowing NullBackend) — CI runs the
  full engine loop against it; offscreen rendering (topic 11 `screenshot`) never
  touches a real window.
- Event-injection harness: scripted resize/mode-switch/input sequences against
  the real backends under a nested compositor (sway headless / Xvfb) in CI —
  resize-storm and mode-flip soak tests, letterbox correctness golden frames.
- DPI matrix tests: scale-factor changes mid-session (monitor drag simulation)
  must not leak wrong-size swapchains.

## Risks

- **Edge-case iceberg** (the reason winit exists): DPI transitions, focus
  semantics, IME, WM quirks. Contained by: Linux-first (daily-driven + CI-tested
  from P0), Win32/AppKit at P5C behind e2e suites against real desktops,
  letterbox-always-works fallback, and the two-mode model cutting the worst
  platform surface (modesetting) entirely.
- **Protocol-from-scratch cost**: wire codecs, keymap handling (XKB parsing),
  and fd-passing plumbing are real work before the first window opens. Contained
  by: subset discipline (implement messages we send/receive, nothing more), our
  own codegen from protocol XMLs, and wayr as the Wayland head start.
- **wayr relationship**: engine needs stay within winit-parity surface;
  absorbing wayr into the no-deps rule is wayr work, gated by its own scope
  process (its 0.2.0 reset exists for a reason).
- **Input latency/correctness** (esports audio pillar implies esports input
  standards): raw motion + pointer lock are P0 features, not afterthoughts;
  input timestamps preserved end-to-end for the P2 input pipeline.

## Corrections (2026-08-09)

- **"File-list transfers ride the same machinery … with zero seam changes" is
  not true.** The clipboard section promises OS-file paste into the editor's
  asset browser for free. On Win32 `MimeType::UriList` is a registered format
  and `CF_HDROP` is never read, so an Explorer copy is invisible, and the shared
  `clipboard::parse_uri_list` cannot round-trip a Windows path. On macOS only
  `public.file-url` is read. On X11 there is no XDND at all. Closing it is seam
  work on three of four backends — see [08-editor.md](08-editor.md)'s correction
  and `docs/backlog.md`.
- **Render scale has a renderer half now, and no seam half.** The display-mode
  table defines borderless as an internal render target upscaled to the native
  surface, `ShellCaps` carries `HW_UPSCALE`, and
  [18-render-features.md](18-render-features.md) orders the post chain around
  the upscale. As of 2026-08-27 that upscale exists: `crcbl_render`'s
  `set_render_scale` and `shaders/upscale.slang` draw the frame at a fraction of
  the caller's extent and reconstruct it, and the post chain really does cost
  what the internal extent says. **What is still missing is everything above the
  renderer** — no settings key reads `render_scale`, no `Shell` carries a
  render-scale request, and no window system is asked to do the resample
  instead. So borderless still renders at native size and `HW_UPSCALE` still
  describes a mechanism nothing can ask for, which is why it stays clear on
  macOS despite `CAMetalLayer` supporting exactly this. The seam addition is a
  render-scale request on `Shell`, and that remains a decision above this crate.
  The settings catalogue above names the key it would be driven by —
  `render_scale`, the internal extent, distinct from `resolution`.
- **Exclusive fullscreen stays dropped, and one backend actively defends it.**
  `crcbl-dx12` calls `MakeWindowAssociation(DXGI_MWA_NO_ALT_ENTER)` per
  swapchain so DXGI's own message hook cannot take Alt+Enter into a fullscreen
  transition nothing above the seam would see. It is a window-global side effect
  a HAL backend arguably should not have; recorded here because it enforces this
  document's locked decision from below the seam.
