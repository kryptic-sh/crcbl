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
  HWND, NSView, canvas id). Shell consumers can't open it;
  `crcbl-vk`/`crcbl-wgpu` can. Mirrors raw-window-handle's role, but ours.
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

Explicitly out (post-MVP or never): exclusive fullscreen, multi-window MVP
(editor is single-window until it isn't), gamepad raw backends
(evdev/XInput/GameController per topic 19 schedule), touch.

## Backends

| Platform | Backend                                                                                                  | Lands |
| -------- | -------------------------------------------------------------------------------------------------------- | ----- |
| Wayland  | libwayland-client connection/proxies + **our** protocol codegen on `wl_proxy_marshal_flags` (wayr donor) | P0    |
| X11      | libxcb connection + **our** request/event layer (core, EWMH atoms, RandR, XKB)                           | P0    |
| Web      | canvas + DOM events via our own minimal JS shim + wasm imports                                           | P5    |
| Windows  | hand-written Win32 FFI (`extern "system"` decls for the surface we use)                                  | P14   |
| macOS    | hand-written Objective-C runtime FFI (`objc_msgSend`) to AppKit                                          | P14   |

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
  functions, not thousands; audited by use. Land with Metal/DX12 (P14) — before
  that they'd be compile-verified-only anyway (gpur lesson: that's not support).
- **Web**: `wasm-bindgen` is avoided in the shell; a small hand-rolled JS glue
  file exports canvas/event/rAF hooks as plain wasm imports. (Whether the rest
  of the wasm build keeps wasm-bindgen is a stage 10 decision — the shell
  doesn't require it.)

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
  from P0), Win32/AppKit deferred to P14 with real-hardware time budgeted,
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
