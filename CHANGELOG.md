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

- **crcbl-shell**: an **AppKit end-to-end pass**, so macOS is held to the
  standard the other three backends already were. It extends
  `crates/crcbl-shell/tests/appkit_session.rs` — the `harness = false` target
  that exists because `libtest` runs every body on a thread it spawns and AppKit
  raises off the main thread — rather than adding a second one, because two
  processes each bootstrapping an `NSApplication` would fight over which is
  frontmost and injected input follows whichever wins.

  **Input the window system generated**, through `CGEventPost`: a key press and
  its release, an arrow key, pointer motion, a click and a wheel notch. That is
  what reaches `interpretKeyEvents:`, which nothing had ever reached — so
  `ShellEvent::TextCommit` on macOS was in exactly the state the Win32 backend's
  was in before its own e2e suite found `TranslateMessage` missing from the
  pump. It also observes the asymmetry `appkit::pointer` exists to describe: a
  cursor moved down the screen comes back with a _larger_ window Y and a
  _positive_ raw delta, because `locationInWindow` is Y-up and Quartz's delta is
  not.

  **A pasteboard round trip against `pbcopy` and `pbpaste`**, in both directions
  — Apple's own processes, with no `crcbl-shell` in them, which is what
  separates "the pasteboard server has the bytes" from "the shell answered its
  own read out of a cache". A helper binary of ours was considered and declined;
  `docs/backlog.md` records that this covers text only, since `pbpaste` cannot
  be asked for the engine's own format.

  **AppKit as the judge** rather than the backend's own bookkeeping, through
  three new `crcbl_shell::session_support` entry points — `window_facts`,
  `key_window` and `resize_window` — and `activation`, which now takes the title
  of the window to describe. Three of the five switches `appkit::view` lists as
  "structural rather than verified" are now read back off the live window —
  `acceptsMouseMovedEvents`, the first responder being `CrcblView` rather than
  the window, and the registered dragged types — and a resize AppKit performed,
  a borderless flip that covers the `NSScreen` it names exactly, and the
  restored title bar are all checked against `NSWindow` and `NSScreen`.

  **None of that readback goes through `-[NSApp keyWindow]` any more**, which is
  the correction the first macOS run forced. That run reported
  `app_active: false` with `can_become_key: true`: a GitHub runner gives an
  unbundled binary a window server and a window but not activation, so the key
  window was nil and every assertion behind it was being discarded over a
  precondition it did not have. `window_facts` finds this process's own window
  by title among `-[NSApp windows]`, and reports `app_active` and `is_key` as
  fields rather than requiring them; `key_window` remains for the one caller
  that genuinely needs the keyboard, which is `CGEventPost`. The harness then
  asks the session for activation itself —
  `-[NSRunningApplication activateWithOptions:]`, which reaches a lever the
  backend is right not to have, since a game does not get to steal the focus —
  and **the runner grants it**, so the window becomes key and the injected input
  runs. If it is ever refused, the injected-input assertions and the warp
  readback are skipped with a printed account of what did not run and why,
  rather than failing the session or going quietly green.

  **A warp is not an event**, which the same run found:
  `CGWarpMouseCursorPosition` moves the cursor and posts nothing, so reading a
  warp back needs a real `kCGEventMouseMoved` posted at the point the cursor was
  moved to. That makes the check stronger than it was — the seam's conversion
  into Quartz's global space and the backend's conversion out of
  `locationInWindow` are now judged against each other, rather than one of them
  against a tracking-area crossing that a boundary-crossing warp happened to
  produce.

  **And a synthesized mouse event carries no delta unless the poster sets one.**
  `CGEventCreateMouseEvent` leaves `kCGMouseEventDeltaX`/`Y` at zero and
  `-[NSEvent deltaX]` reads exactly those, so `raw_delta` came back `(0.0, 0.0)`
  — correctly. The harness now writes a known delta onto the event, so the seam
  is held to reporting _that_ pair rather than merely something non-zero, and
  the asymmetry `appkit::pointer` exists to describe is observed for the first
  time: a move right and **up** comes back with a larger window X, a smaller
  window Y, and a delta whose Y is still negative, because `locationInWindow` is
  flipped into the seam's space and Quartz's delta is already in it.

  **`ShellEvent::TextCommit` from a real keystroke now has executable coverage
  on macOS.** The injected `kVK_ANSI_A` reaches `interpretKeyEvents:` through
  `sendEvent:` and the first responder, and commits `"a"` — the chain that was
  written blind and is the macOS counterpart of the `TranslateMessage` gap the
  Win32 backend shipped with. That also settles the risk the slice was written
  around: **TCC does not gate `CGEventPost` for events delivered back to the
  posting process.**

  **And the scroll notch reaches the event it is posted on.**
  `CGEventCreateScrollWheelEvent`'s `wheel1` is a _named_ parameter — only
  `wheel2` and `wheel3` are variadic — and the harness had declared the `...`
  one parameter early, so on Apple silicon the amount went to the stack while
  the callee read a register and the event scrolled by zero. The same class of
  defect `appkit::ffi` guards against for `objc_msgSend`, arriving through a
  hand-written C variadic instead.

  **The sample-level pass has no macOS equivalent**, on the same terms as
  Windows: it needs a renderer and macOS has no Vulkan until MoltenVK clears its
  P14 gate. `docs/plan/ROADMAP.md`'s 2026-08-04 correction says so, and
  `docs/backlog.md` carries it as a gap rather than approximating it.

- **crcbl-shell**: **the clipboard and file drops on the AppKit backend**, so
  `ShellCaps::CLIPBOARD` and `ShellCaps::DRAG_DROP` are set there and
  `clipboard_offer`/`clipboard_request` answer instead of returning
  `Unsupported`. macOS is now the fourth backend to implement the whole seam.

  A copy publishes every offered format at once under its own `NSPasteboard`
  type: text under `public.utf8-plain-text`, which is what TextEdit and every
  other application reads, and the engine's own `application/x-crcbl+ron` under
  that mime string verbatim — the same spelling the other three backends use, so
  an engine-to-engine copy is lossless and byte-identical across platforms. An
  empty offer slice **clears** the pasteboard, because macOS has no owner to
  release: a pasteboard is content the server holds. Reads answer the three
  `ClipboardContent` outcomes distinctly, and the answer names the format that
  was _asked_ for — a pasteboard type is a UTI rather than a mime, so there is
  no peer spelling to report.

  Nothing is provided lazily and nothing is held after a write:
  `setData:forType:` copies the bytes to the pasteboard server, so this backend
  carries no deadline, no retry budget and no state between pumps — the only one
  of the four whose clipboard needs none of them.
  `pasteboard:provideDataForType:` is refused for the same structural reason the
  Win32 backend refuses `WM_RENDERFORMAT`, and `docs/backlog.md` says not to
  revisit it without a seam change.

  File drops arrive through `registerForDraggedTypes:` and the
  `NSDraggingDestination` methods on the content view, honouring
  `WindowDesc::accept_drops` — and there the gate is the **system's**: AppKit
  sends no dragging message at all to a view that has not registered, which is
  the same strength as Win32's `WS_EX_ACCEPTFILES` and stronger than Wayland's.
  Each `public.file-url` goes through the shared `parse_uri_list`, so a
  percent-encoded name, a `file://localhost/…` authority and a filename that is
  not valid UTF-8 all behave exactly as they do on the other backends, and a
  dragged _URL_ is not turned into a path that looks plausible and does not
  exist. Promised files (`com.apple.pasteboard.promised-file-url`) are not
  accepted; the seam has no way to name a destination for one.

- **crcbl-shell**: **input on the AppKit backend** — keyboard, text, pointer,
  scroll, relative motion, pointer lock, cursors and warping, so a game is
  playable on macOS rather than merely windowed. `POINTER_LOCK`, `POINTER_WARP`,
  `RAW_POINTER_MOTION` and `TEXT_IME` join the capability set, and
  `ShellCaps::has_mouselook()` is true there.

  Keys carry Apple's `kVK_*` codes mapped to `KeyCode` (a third numbering, which
  coincides with neither evdev nor PS/2 set 1 at any point), an X11 keysym, the
  auto-repeat flag and the modifiers of that event. **Four keys the seam names
  are unreachable on macOS** — `PrintScreen`, `ScrollLock`, `Pause` and
  `ContextMenu` have no `kVK_*` code, and those positions on a Mac keyboard are
  `F13`–`F15`, which are their own keys. **Num Lock is not a modifier there**:
  macOS has no such latch, and `NSEventModifierFlagNumericPad` means "this key
  is on the keypad", so `Modifiers::NUM_LOCK` is never set. **Option is reported
  as `ALT` and never `ALT_GR`**, because the same key is macOS's Alt and its
  level-3 shift and no third key distinguishes them — the opposite conclusion
  the Win32 backend reaches, from the same starting point.

  Text goes through a real `NSTextInputClient` and `interpretKeyEvents:`, so
  commits arrive from the **input method** and dead keys compose — reading
  `-[NSEvent characters]` instead would leave every accented character
  unreachable. Pre-edit is tracked and never surfaced (the seam has no event for
  one), so an input method's candidate window appears at the window's origin
  rather than under a caret.

  The pointer reports both scroll units — a trackpad's `ScrollDelta::Pixels` and
  a wheel's `Lines`, the first backend where both arms are reachable — buttons
  past the fifth through `otherMouseDown:`, and enter/leave from an
  `NSTrackingArea`. `PointerMode::Locked` freezes the cursor with
  `CGAssociateMouseAndMouseCursorPosition(false)` and needs none of the
  clip-and-recentre machinery Win32 and X11 carry.

  Two things a consumer must know. **`PointerMode::Confined` is refused,
  permanently**: macOS has no confine API, only warping the cursor back after it
  has already left, so `POINTER_CONFINE` stays clear — the only desktop backend
  where the two capture modes come apart. And **`RAW_POINTER_MOTION` here is
  unclamped but _accelerated_**: `NSEvent`'s deltas are separate from the
  absolute position and keep flowing at the screen edge, which is what makes a
  camera work, but macOS publishes no way to remove the system's pointer
  acceleration from them.

- **crcbl-shell**: an **AppKit backend**, registered and selected automatically
  on macOS — so `crcbl_shell::open()` now returns a real window there instead of
  `NoBackend`. The window lifecycle: `NSApplication` bootstrap, create, show,
  hide, destroy, title, close-request interception (`windowShouldClose:` answers
  `NO`, and the seam asks), windowed ↔ borderless on a **named** display with
  the windowed style mask and frame restored exactly, size constraints through
  `setContentMinSize:`/`setContentMaxSize:`/`setContentAspectRatio:`, `NSScreen`
  enumeration with visible frame, backing scale, refresh rate and hotplug, an
  event pump and a blocking `wait_events`, and `SurfaceTarget::AppKit` for the
  HAL. Built on hand-written Objective-C runtime FFI — `objc_getClass`,
  `sel_registerName`, `objc_msgSend` and runtime-built classes — with no `objc2`
  and no `cocoa`.

  The shell creates and owns the `CAMetalLayer` and hosts it on its `NSView`, so
  `SurfaceTarget::AppKit` carries the layer and **no HAL backend ever touches
  AppKit**. Borderless is a frameless window at the display's size, not
  `toggleFullScreen:`: the desktop's mode is untouched and there is no Spaces
  transition. `ASPECT_HINT_HONORED`, `WINDOW_POSITION`, `SERVER_DECORATIONS`,
  `MULTI_WINDOW` and `EVENT_WAIT` are set; the pasteboard and drag-and-drop are
  the slice after this one and every bit they would set stays clear.

  Four macOS facts a consumer may need. **`AppKitShell::open` requires the
  process's main thread** and returns `ShellError::Backend` naming that rule
  anywhere else — AppKit raises an Objective-C exception otherwise, which
  unwinding into Rust is undefined behaviour. **`FRACTIONAL_SCALE` is clear**,
  because `backingScaleFactor` is 1.0 or 2.0 and a "scaled" HiDPI mode changes
  the point resolution rather than the factor. **`MonitorInfo::bounds` does not
  tile** across displays of different scales, because AppKit's global coordinate
  space is points rather than pixels — the caveat that field already documents
  for Wayland, now true on a second platform; window placement is unaffected,
  because it is expressed in points. And `MonitorInfo::refresh_millihertz` can
  finally be non-integral: `CGDisplayModeGetRefreshRate` reports 59.94 as 59.94,
  which no other backend's API is able to.

- **crcbl-shell**: a **Win32 backend**, registered and selected automatically on
  Windows — so `crcbl_shell::open()` now returns a real window there instead of
  `NoBackend`. The window lifecycle: create, show, hide, destroy, title,
  close-request interception, windowed ↔ borderless on a named monitor with the
  windowed placement restored exactly, size constraints (`WM_GETMINMAXINFO`
  limits and a live `WM_SIZING` aspect lock), monitor enumeration with work
  area, refresh rate and per-monitor DPI, per-monitor-v2 DPI awareness with
  `WM_DPICHANGED` handled mid-session, a message pump, a blocking `wait_events`,
  and `SurfaceTarget::Win32` for the HAL. Built on hand-written
  `extern "system"` declarations for `user32`, `gdi32`, `shcore` and `kernel32`
  — there is no `windows-rs` and no `winapi`.

- **crcbl-shell** (Win32): **input**. Keyboard events carry a PS/2 set-1 scan
  code with its `E0` prefix folded in, the `KeyCode` for that physical position,
  the layout's `Keysym`, the modifiers and the auto-repeat flag; `WM_CHAR`
  becomes `TextCommit`, with surrogate pairs reassembled so an astral codepoint
  arrives whole and control characters dropped. The pump calls
  `TranslateMessage`, which is what makes a `WM_CHAR` exist at all — dead keys,
  AltGr and an input method's commit all arrive through it, and without it
  typing into a Crucible window produced no text whatever. Pointer motion, all
  five buttons including the two thumb buttons, derived enter and real leave,
  mouse capture so a button released outside the window is still reported, and
  both wheel axes with high-resolution fractions of a detent preserved.
  `WM_INPUT` raw relative motion, with an absolute-reporting device — a
  remote-desktop session, a tablet — differenced into a delta instead of being
  read as one. `PointerMode::Confined` and `PointerMode::Locked` through
  `ClipCursor`, and `warp_pointer` through `SetCursorPos`. Cursor shapes are the
  stock `IDC_*` set applied from `WM_SETCURSOR`, and hiding goes through a
  balanced `ShowCursor` count.

  A confined pointer's clip is the client rectangle **intersected with the
  virtual screen**: `ClipCursor` clamps, so a window larger than the desktop is
  confined to the part of itself that is on screen.

  `POINTER_LOCK`, `POINTER_CONFINE`, `POINTER_WARP` and `RAW_POINTER_MOTION` are
  now set on this backend — the last of them latched on whether
  `RegisterRawInputDevices` was accepted — and `set_cursor` applies rather than
  records. **`TEXT_IME` stays clear**: nothing here touches `WM_IME_*`, so there
  is no composition string and no candidate-window placement, and typing working
  through `WM_CHAR` is not the same claim.

  Three Windows facts worth knowing before building on it: a window frozen
  during a user drag-resize is the system's modal message loop and not a hang; a
  monitor's refresh rate is a whole hertz here, so 59.94 Hz reports as 60; and a
  `DeviceId` names a device _kind_ rather than a device, so two mice cannot be
  told apart yet.

- **crcbl-shell** (Win32): **the clipboard and file drops**, so `CLIPBOARD` and
  `DRAG_DROP` are now set and `clipboard_offer`/`clipboard_request` work instead
  of returning `Unsupported`.

  A write publishes each offered format at once — `CF_UNICODETEXT` for
  `text/plain;charset=utf-8`, and a `RegisterClipboardFormatW` format named
  after the mime for everything else — so one copy reaches Notepad as text _and_
  round-trips through another Crucible as `application/x-crcbl+ron` without
  loss. The reader picks. Windows synthesizes `CF_TEXT` and `CF_OEMTEXT` from
  the Unicode text in both directions, so there is no `TARGETS`-style format
  negotiation to do. An empty `offers` slice empties the clipboard: Win32 has no
  selection _owner_ to relinquish, so that is what "release" can mean here.

  Reads are answered inside `clipboard_request` and delivered on the next
  `pump`, exactly once. `Win32` has neither Wayland's focus gate nor its serial
  requirement — any window may open the clipboard at any time — so a read is
  never _held_ and `clipboard_offer` never returns `NeedsUserInteraction`. The
  one real wait is `OpenClipboard` being refused while another process has the
  clipboard open, which is routine; it is retried for a bounded 70 ms and then
  reported `Unavailable` rather than failing a paste over a refusal that was
  over before the user noticed.

  Files dropped on a window created with `WindowDesc::accept_drops` arrive as
  one `ShellEvent::DroppedFile` per file, with the drop point in client pixels,
  through `DragAcceptFiles` and `WM_DROPFILES`. The gate is enforced by the
  system as well as by this backend: without `WS_EX_ACCEPTFILES` no drop message
  is ever sent. **There is no drag feedback** — no drop cursor and no hover
  highlight while a file is still in the air — because that is `IDropTarget`,
  which is COM; the drop itself works.

- **crcbl-shell** (Win32): `wait_events` now drains the message queue before it
  sleeps and no longer passes `MWMO_INPUTAVAILABLE`. A message _sent_ to a
  window (rather than posted) leaves `QS_SENDMESSAGE` set after `PeekMessage`
  has dispatched it, and that flag asks to be woken by exactly that bit — so the
  wait returned immediately, forever, and an application idling at zero frames
  per second span a core instead. Draining first is the stronger form of what
  the flag was there for. That removed `QS_SENDMESSAGE` from the picture and did
  not make the wait sleep on a CI runner, where a _posted_ message still wakes
  it; `docs/backlog.md` carries what is known and what is not.

- **crcbl-shell**: `DisplayMode::satisfied_by`, the request-versus-answer
  comparison `WindowState::mode_request_honoured` now uses.

- **crcbl-shell**: a **Win32 end-to-end suite** behind the new `win32-e2e`
  feature (off by default), run by `crates/crcbl-shell/tests/run-win32-e2e.ps1`
  and by a CI job of its own against a real Windows desktop — the treatment
  Wayland and X11 got at P0.5/P0.6. It drives the backend through `open_backend`
  and `dyn Shell` only, and covers what no in-process test can reach:
  keystrokes, clicks and wheel notches **injected from another process** with
  `SendInput`, so they arrive as posted, queued, translated and dispatched
  messages; mode flips and resize storms judged by `GetWindowRect` rather than
  by the backend's own bookkeeping; monitors, DPI and focus against the desktop
  the machine actually has; and a clipboard round trip with a second process, in
  both directions, with this shell's message loop stopped.

  Two helper binaries come with it, `crcbl-e2e-win32-input` and
  `crcbl-e2e-win32-clip`, on the same terms as the two Linux key senders:
  `required-features`, and a `main` that fails loudly on any other platform.

  **The harness defeats Windows' foreground lock, and the backend does not learn
  how.** `SetForegroundWindow` is granted only to a process that already owns
  the foreground or received the last input event, and under `nextest` every
  test is a fresh process with neither — so three tests spent twenty seconds
  each being refused by the job's own console window. The suite now lowers
  `SPI_SETFOREGROUNDLOCKTIMEOUT` for the session (restoring it on the way out,
  for a desktop that is not a CI runner) and attaches its input queue to the
  foreground thread's around the request, which is what an automated harness
  does to arrange a precondition a human would have arranged by clicking. None
  of it is in `src/win32/`: a game does not get to steal focus, and a backend
  that knew how could do it to a user.

  **The sample-level pass has no Windows equivalent yet.** The Linux suites
  press F11 at a running game, which needs a renderer, and no runner on this
  platform has a Vulkan device — `docs/plan/ROADMAP.md` schedules it for P14.

### Changed

- **crcbl-shell** (Wayland): the effective mode of a fullscreen window now names
  the monitor it is on, taken from `wl_surface.enter`. Asking for a monitor is
  only a hint on this platform, but which one the compositor used is observable,
  and without it `mode_request_honoured` answered "no" to a request the
  compositor had honoured exactly. A summary line that read `borderless` may now
  read `borderless on monitor 2`. `None` still means the backend cannot say —
  the surface is on no output or on two.

### Fixed

- **crcbl-shell** (AppKit): `CrcblWindow` overrides
  `constrainFrameRect:toScreen:` to answer the proposed rectangle unchanged, so
  AppKit can no longer silently rewrite a frame this backend sets. The default
  keeps a title bar clear of the menu bar, which is right for a window a person
  dragged and wrong for every frame here — all of them are computed from an
  `NSScreen` rectangle and are on that screen by construction. `setFrame:` also
  now reads the frame back and logs when a window did not go where it was put,
  which nothing above this layer could otherwise notice: `WindowState` carries
  an extent and no position.

  That override was necessary and not sufficient; the defect that prompted it is
  fixed in the entry below.

- **crcbl-shell** (AppKit): **a mode change put the window back where it was
  created.** `DisplayMode::Borderless` produced a window of exactly the right
  size at the wrong origin — hanging off two edges of the display — and the way
  back was worse, restoring the creation frame's origin _and size_ rather than
  the placement the window had before the flip. Neither was visible through the
  seam, which carries an extent and no position.

  The cause is a fact about AppKit worth stating on its own:
  **`-[NSApplication setPresentationOptions:]` returns every window of the
  application to its creation frame.** Not the window it is called about — the
  property is on `NSApplication` — and not "constrains it to the screen". The
  backend applied the borderless presentation options _after_ placing the
  window, so every frame it set was immediately thrown away, on both legs of the
  round trip.

  `apply_mode` now applies the style mask, then the presentation options, then
  the frame, making the frame the last geometry it sets. The middle position
  matters as much as the last: applying the options before the style mask
  changes makes AppKit raise `NSInvalidArgumentException`, and an Objective-C
  exception unwinding through Rust aborts the process. `appkit::window`'s module
  docs carry the measurement and all three positions, since anyone reordering
  those statements would otherwise reintroduce one defect or the other.

- **crcbl-shell** (AppKit): **a mode change took the keyboard away from the
  view.** `-[NSWindow setStyleMask:]` rebuilds a window's frame view and the
  content view stops being the first responder — so after a flip to
  `DisplayMode::Borderless`, or back, `sendEvent:` delivered every key event to
  the window and `CrcblView` received none. A game that pressed F11 went
  permanently deaf, silently, with no error anywhere. `apply_mode` now re-claims
  the first responder after each style-mask change, sharing `focus_content_view`
  with window creation so the two cannot drift, and the session asserts the view
  still has the keyboard **after the borderless leg** as well as after a full
  round trip — a game stays borderless, so a responder restored only on the way
  out would be a game that is deaf for as long as it is being played.

- **crcbl-shell** (AppKit): windows no longer take part in **macOS state
  restoration**. `isRestorable` defaults to `YES`, which enrols a window in a
  feature this backend cannot honour and should not want: restoration re-creates
  windows at launch through a `restorationClass` or an application-delegate
  callback, neither of which exists here — the backend deliberately never takes
  the delegate slot — and it makes the operating system a second, invisible
  source of truth for a placement the seam hands to `WindowDesc` and a game
  hands to its settings screen. It also writes saved state to disk keyed by an
  application identity an unbundled binary does not stably have.
  `setRestorable:` is now `NO` at creation. Argued on its own merits; whether it
  also accounts for the borderless-origin defect above is a separate question.

- **crcbl-shell** (X11): hiding a window with `set_visible(false)` unmapped it
  without telling the window manager. ICCCM 4.1.4 requires a synthetic
  `UnmapNotify` to the root alongside the unmap, because a reparenting manager
  watches the frame it created rather than the client window inside it and may
  never see the real event. Under `openbox` the window was unmapped and remapped
  before the application could observe it hidden.

- **crcbl-shell**: `WindowState::mode_request_honoured` compared the requested
  and effective modes with `==`, which is wrong whenever the backend can name
  the monitor. `Borderless { monitor: None }` means "wherever the window already
  is" as a _request_, so an answer of `Borderless { monitor: Some(..) }`
  satisfies it — but the two are not equal, so every granted fullscreen on X11
  read as refused and a UI toggle over a fullscreen window would have shown
  "off". The comparison is now `DisplayMode::satisfied_by`, which keeps the
  asymmetry: a request naming a monitor is still not answered by one that cannot
  say which.

- **crcbl-shell** (X11): the backend never wrote `WM_HINTS`, so it never told a
  window manager that its window wants the keyboard. ICCCM 4.1.7 lets a window
  manager assume convenient values when the property is absent, and "this window
  takes no input" is one of them — a game whose window is never focused receives
  no key for its whole run. It now writes `input = True` with `NormalState`,
  which is ICCCM's passive focus model and what every toolkit does. **Changed
  nothing measurable under openbox**, which defaults the other way; this is
  conformance rather than an observed repair.

- **crcbl-shell** (X11): a `set_mode` issued after a window was configured but
  before its `MapNotify` arrived was silently dropped. `apply_fullscreen` chose
  between writing `_NET_WM_STATE` and sending a `ClientMessage` on whether the
  window was mapped — which follows `MapNotify` — but a window manager begins
  managing a window at the map _request_, and on X11 the first configure also
  arrives before `MapNotify`. A game that opened a window, waited for its size
  and asked for fullscreen landed in that gap every time: it wrote a property
  the window manager then overwrote with its own view. It now branches on
  `XWindow::map_requested`, and the whole X11 suite runs under `openbox` in CI
  as well as under bare Xvfb.

- **crcbl** (`engine`): a run that ended because the player closed the window
  reported `DisplayMode::Windowed` whatever mode it had been in. Accepting a
  close request destroys the window, and the summary is built afterwards, so
  `ModeRequest::mode` had nothing left to read and fell back to its default — in
  the same words a genuinely windowed run uses, so nothing downstream could tell
  the two apart. `ModeRequest` now records the mode it last saw and the new
  `ModeRequest::mode_at_exit` prefers the live answer, falling back to that.
  `Loop::finish` and `apps/bare` both use it.

- **crcbl-shell** (X11): a window created with
  `WindowDesc { mode: Borderless, .. }` reported its own request back as the
  effective mode when no window manager was running. EWMH has the _client_ write
  `_NET_WM_STATE` to request an initial state — before a window is mapped there
  is no window manager conversation to have — and a window manager then takes
  ownership of the property. The backend worked out the effective mode by
  reading that property back, so with nobody to take ownership it read its own
  write: `effective_mode()` said borderless and `mode_request_honoured()` said
  true, for a window still at its windowed size that nothing had touched. It now
  trusts `_NET_WM_STATE` only when `_NET_SUPPORTING_WM_CHECK` says something is
  there to have written it.

  `set_mode` after mapping was never affected — that path sends a client message
  to the root window and never writes the property — so the bug was reachable
  only through the creation path, which is exactly the path the new
  `--fullscreen` flag takes. Every WM-less X session, kiosk and CI runner would
  have had a summary line claiming a fullscreen it did not have.

### Added

- **crcbl** (`args::Common`): `--fullscreen`, and `Common::display_mode()` that
  turns it into a `DisplayMode` for `WindowDesc::mode`. Asked for at window
  creation rather than switched to afterwards, so a fullscreen game does not
  show a decorated window for the frames a `set_mode` would take to land. `F11`
  still toggles from either starting point. Every sample honours it —
  `apps/sandbox` through its own parser, which predates the shared one.

- **samples**: the summary line each binary prints now names the display mode
  the window system actually settled on, beside the extent. `RunSummary::mode`
  already carried it and nothing reported it, which left a refused fullscreen
  indistinguishable from an honoured one from outside the process. `apps/bare`
  gained a `Summary::mode` field to do the same from a hand-written loop, via
  the public `engine::ModeRequest::mode`.

- **crcbl-sprite** (`bake::bake_dir`): the generated table now declares
  `ART_TICK_HZ`, the rate the holds were baked at. A `.crpix` counts holds in
  simulation ticks and an Aseprite sidecar counts milliseconds, so the
  conversion runs once at bake time and once at load time and the two must agree
  — and a build script cannot `use` the crate it builds, so every consumer
  declared the number a second time beside its loader. Five copies (`apps/*` and
  `crcbl-render`) are deleted; the `build.rs` value is the only source.

- **crcbl-phys**: `DampingForce::world_force(velocity, mass, dt)` and
  `DragForce::world_force(velocity)`, beside the `ThrustForce::world_force` that
  already existed. A force provider applies to **every** dynamic body, so a game
  damping one entity among a field of others could not use the pipeline;
  `apps/asteroids` wrote `-k·v` and the `mass/dt` cap out by hand instead, and
  that copy is now deleted. The cap travels with the route — it is what stops a
  coarse tick rate from over-damping past zero and flying the body backwards.

- **crcbl-phys**: `overlap_sphere_into` on both `PhysicsSystem` and
  `PhysicsWorld`, and `Bvh::traverse_aabb_into`, so a game that queries once per
  body per tick can hoist one buffer out of its loop. The owned forms cost three
  `Vec`s per call — the result, the collider ids, and the BVH's candidate list —
  and the descent stack a fourth; the `_into` path clears and refills the
  caller's buffer and keeps the rest as fields, so a crowd steers without
  allocating. The owned forms remain, unchanged for every existing caller, and
  now delegate.

- **crcbl-phys**: `PhysicsSystem::body_mut(entity) -> Option<&mut RigidBody>`,
  for a game that chooses a velocity rather than having one integrated onto it.
  `set_body` was the only writer and it costs two hash operations — an insert
  into the body map and a touch of the transform map — to change one `DVec3`,
  which a crowd pays once per agent per tick; `apply_force` is not an
  alternative, because a kinematic body's zero inverse mass makes a force a
  no-op. It cannot move a collider: position lives in the transform, and
  `set_transform` is still what tells the broadphase.

- **crcbl-render** (`sprite_pass`): `batch_count(&[Sprite]) -> usize` answers
  how many draw calls a sprite list will cost, without a device. The batching
  rule — a run of consecutive sprites naming one sheet is one draw, so `A A B A`
  is three and not two — was previously readable only by writing it out again,
  which `apps/horde` did to put the number on its debug panel. It delegates to
  the batcher the pass itself uses, so it cannot drift from it.

- **crcbl**: the simulation half of the engine is re-exported, so a game names
  `crcbl` and the standard library and nothing else. `crcbl::ecs`,
  `crcbl::phys`, `crcbl::net`, `crcbl::server`, `crcbl::client`, `crcbl::input`,
  `crcbl::audio`, `crcbl::store` and `crcbl::sprite` join the graphics stack
  that was already there, and `crcbl::log` re-exports the logging facade — its
  macros resolve through `$crate`, so `crcbl::log::info!` expands exactly as
  `log::info!` does and no wrapper macro exists.

  The umbrella's headline claim has been "one dependency for a game" since it
  was written, and until now only `apps/sandbox` could keep it: the other four
  samples each named eleven workspace paths beside it. None of the nine crates
  depends on `crcbl`, so this is nine `pub use` lines rather than a
  restructuring — the arrows already pointed this way and nobody had drawn them.

  `crcbl::sprite` is the reader (`load`), never the encoder. A build script that
  bakes art still names `crcbl-sprite` itself with its `bake` feature, which is
  the one dependency a sample continues to spell out, and is what keeps a PNG
  encoder out of a shipped binary.

- **crcbl** (`crcbl::engine`): `Pending` folds the whole of a pump batch that
  belongs to the loop rather than the game — the pointer, focus loss, and the
  three reserved keys `DEBUG_OVERLAY_KEY` (F3), `PAUSE_KEY` (Escape) and
  `FULLSCREEN_KEY` (F11), which are now the engine's constants. `observe`
  returns `Handled::Loop` or `Handled::Game`, so a sample's pump closure is a
  guard and its own key handling; `Pending::carrying` starts a batch from where
  the last frame left the cursor.

  The pointer half was **byte-identical in all four samples**, and it is not
  trivial code: it carries the last position across frames because motion and
  buttons arrive as separate events and a click carries a position only on some
  backends. The reserved keys were three constants spelled out five times, and
  they are the engine's because the thing F3 opens is the engine's.

  196 code lines out of the four `app.rs` files. What is left there is the loop
  — the fixed-step accumulator, teardown, the summary — which is still four
  copies.

- **crcbl** (`crcbl::args`): the flags every sample has. `Common` holds
  `--headless`, `--frames`, `--tick-hz`, `--backend` and the debug-overlay pair,
  with `frame_budget` and `debug_overlay_visible` on it; `Common::consume`
  offers one argument to that set and answers `Yes`, `Help`, `Bad(message)` or
  `No`. `Invocation<T>` wraps a game's own options, `COMMON_OPTIONS_HELP` and
  `COMMON_TAIL_HELP` are the shared `--help` blocks, and `positive`/`number`
  parse a flag's value with the rejection wording the samples already used.

  **Offered, not imposed.** A game keeps its parse loop and its `Options`
  struct, and claims what `consume` hands back — which is how `--seed`,
  `--max-enemies`, `--prefill` and `--wall-clock` stay per-game, and how
  `apps/sandbox` goes on taking `--camera` and `--title` while not being a
  consumer of this at all.

  The four game parsers were the same file: flappy's and asteroids' differed in
  **eight lines**, six of them usage prose. 894 code lines across the four
  became 599 against 270 in the engine, and the flags themselves are now tested
  once rather than four times. Each sample keeps one test that the engine's
  cannot make — that its parser actually _calls_ `consume`, since one that
  forgot would pass every test in `crcbl::args` and still reject `--headless`.

  The drift this closes was real: three of the four parsers had dropped
  breakout's assertion that the default backend stays `None`, which is what
  stranded CI on a machine with no driver. Each sample's `USAGE` now asserts it
  contains both shared help blocks byte for byte, so a reworded flag description
  reddens the build instead of shipping.

- **crcbl-store** (`crcbl::store::record`): `Record`, one `u32` kept between
  sessions. `Backing` picks where — `None` for a headless run that must leave no
  trace, `Backing::config(app)` for the platform's config directory, and
  `Backing::Browser` for a store the page's shim installed. `raise` writes only
  when the new value is larger; `set` is for the game whose better is smaller.

  The crate handed out a `StorageSource`, an atomic write and a
  platform-standard root and stopped there, so every sample that wanted a high
  score wrote the platform arms, the little-endian encode, the corrupt-file case
  and the headless rule itself. Four did, and the bodies matched line for line
  under names that agreed about nothing — `HighScore` in `high_score.bin`,
  `Best` in `best.bin`, and horde's `Best` whose number is a run length rather
  than a score. 987 lines of sample code became 389, and what is left is the
  part the engine could not have guessed: which directory, which file name, and
  which browser store.

- **crcbl** (`crcbl::session`): `Loopback`, the single-player session. Pairs an
  in-memory transport, builds the `Server` on one end and the `Client` on the
  other with the same tick rate and the same `ProtocolCompatibility`, hands the
  server its `GameModule`, and spends both clocks' first update at time zero.
  `tick_period`, `server`/`server_mut`, `client`/`client_mut` and `both_mut`
  reach the halves.

  "Single-player is a loopback server" is the engine's architectural decision —
  it is why `crcbl-server` and `crcbl-client` exist at all — and until now
  nothing in `crcbl` expressed it, so all four games implemented it from
  scratch. What stays the game's is what genuinely is: its
  `ProtocolCompatibility`, whose `schema_hash` is what stops one game's client
  hand-shaking with another's server, and its `GameModule`. Neither has a
  default, because a default for either is the wrong answer quietly.

  The baseline update at time zero is the subtle half. A `FrameClock`
  establishes itself on its first update and runs no ticks for it; doing that at
  construction is what lets a game's `tick` promise that every later call runs
  exactly one. Left to the caller, the first frame of the game silently
  simulates nothing.

- **crcbl-audio** (`crcbl::audio::synth`): waveform generators. `sine` for a
  one-shot beep, `looped_sine` for a tone that joins to itself, `noise_burst`
  for a decaying impact, and `fade_gain` for the click-free envelope under the
  first and last. Deterministic: `noise_burst` draws from a caller-supplied seed
  through `crcbl_core::rand`, so the sound a build ships is the sound every
  build ships.

  The crate had a mixer, a sound bank, an output stream and a spatial cue
  grammar, and no way to make a _sound_ — so all four samples wrote one. `sine`
  and its fade helper were byte-identical in flappy, asteroids and horde;
  breakout had the same pair under the names `gen_sine` and `fade_env`.

  Three functions, not a synthesiser: no envelope generator, no filter bank, no
  configurable oscillator type. Three is what the four samples between them
  actually use. Horde's swept `rise` has one caller and stays in horde, now
  built on `synth::fade_gain` and `synth::TONE_AMPLITUDE` so its level cannot
  drift from the engine's.

  **Nothing about the shipped audio changed** — the generators were adopted
  verbatim, and the sample buffers were compared to the engine's element by
  element before the copies were deleted.

- **crcbl** (`crcbl::engine`): frame pacing. `FrameLimit` caps how fast a
  real-time loop runs — a thousand frames a second by default, which is a
  runaway guard rather than a pacing policy, and `Clock::set_limit` changes it.
  The limiter lives on the clock rather than in the loop because every sample
  already calls `Clock::advance` once a frame, so a game gets it without asking;
  and because a manual clock has no wall clock to wait against, a headless run
  is unpaced **by construction** rather than by a check somebody has to
  remember.

  `Pacing` — `Vsync`, `Adaptive` or `Off` — replaces the hard-coded present-mode
  preference and is set through `GpuContextDesc::pacing`. One value rather than
  two flags, so "vsync on, adaptive sync on" is a state that cannot be written
  down instead of one the engine rejects at run time.

  **Nothing here turns adaptive sync on**, and that is not an omission: VRR is
  negotiated between display, driver and compositor, and an application never
  enables it. What changes is what presenting means — on a VRR panel the present
  does not wait for a fixed vblank, the panel follows the presents — so the
  engine's job is choosing a present mode and then staying inside the panel's
  range, which is what the limiter is for. Whether a panel is _actually_ running
  variable-refresh needs `VK_EXT_present_timing`, which is provisional and has
  no bindings in the pinned `ash`; until then `Adaptive` is a request rather
  than an observation.

- **crcbl** (`crcbl::engine`): `Loop`, the frame owned by the engine, and
  `HostedGame`, the seam a game reaches it through. `Loop::frame` pumps the
  shell, routes the input, runs the ticks the clock owes, draws and presents;
  `HostedGame` is the six things that genuinely differed between five samples —
  `menus`, `tick`, `key_event`, `menu_action`/`apply`, `menu_kind`, `draw` — and
  `summary`, which adds a game's own fields to the shared `RunSummary`.
  `FrameInfo` tells a `draw` what its frame did, and `LoopConfig` carries the
  three values that come from the command line rather than the game. `Loop`
  implements `GameLoop`, so `drive` and `crcbl::web::App` step it unchanged.

  `GameGpu` is the frame's half of a game's GPU bundle — `atlas`, `set_menu`,
  `take_draw_list`, `timings`, `frame`, `destroy` — and all five samples already
  had every one of them, with these signatures, as inherent methods.

  **`HostedGame` is not `crcbl::ecs::GameModule`.** That one is the simulation
  the server hosts and a wasm binding will have to reproduce bit for bit; this
  one is the presentation the loop hosts. A game implements both.

  `PolledGpu`'s `extent` and `resize` move to a new `GpuSurface` supertrait,
  which `PolledGpu` and `GameGpu` both require — the same two questions, asked
  by start-up and by the running frame, and declaring them twice on one type is
  how the two answers drift apart. The four samples with a browser build split
  their existing `impl` accordingly; nothing else changes for them.

  `apps/bare` never adopts it: it is the guard that the library path —
  assembling `GpuContext`, `Pending` and `FrameBudget` by hand — keeps working,
  and `crates/crcbl/tests/library_seam.rs` is what proves it from outside the
  crate.

  585 lines of engine and 343 of fixture and tests, against a `FakeGpu` that
  counts presents and a `FakeGame` that records what the loop asked of it —
  including an assertion that the loop never asks a game about a reserved
  `WidgetId`, which is what would silently re-point a resume button.

### Changed

- **crcbl-cli** (`crcbl new`): the scaffold now hands you the engine-owned loop.
  `src/main.rs` was 276 lines that opened the shell, called
  `unsafe { instance.create_surface(&target) }` itself, configured its own
  swapchain and ran its own `loop {}` — while every sample had stopped doing any
  of that and no crate under `apps/` contains an `unsafe` block at all. Its doc
  comment argued the loop was "deliberately yours rather than the engine's"
  because "an engine that owned it could not run in a browser", which
  `crcbl::web` had already disproved in four published demos.

  A generated project is now a `HostedGame` and a `GameGpu` over
  `crcbl::engine::GpuContext`, and arrives with a pause menu on `ESC`,
  fullscreen on `F11`, the debug panel with per-pass GPU timings on `F3`, mouse
  and keyboard menu navigation, and resize handling — none of which the old
  template had. It parses its flags with `crcbl::args::Common` and builds its
  help text from the engine's own two blocks, so `--tick-hz`, `--backend` and
  the debug-overlay pair work and cannot drift. `log = "0.4"` is gone from the
  generated manifest: `crcbl::log` covers it, so a new project starts with the
  same single dependency the samples have. The template ships three unit tests
  and `crcbl-cli`'s scaffold e2e now runs them.

  One consequence to know about: a generated project goes through
  `crcbl::backend`'s real registry, where the old template hardcoded
  `NullInstance`. That registry never falls back to null on its own, so the
  generated `.github/workflows/ci.yml` names `--backend null` — a stock CI
  runner has no driver, and without it the first push fails with
  `ERROR_INCOMPATIBLE_DRIVER`. Drop the flag once that job installs one.

  The library-style loop is still supported and is still `apps/bare`, guarded
  from outside the crate by `crates/crcbl/tests/library_seam.rs`. What changed
  is which of the two a new project starts from.

- **breakout**: the first game hosted by `crcbl::engine::Loop`. `Breakout` is
  seven `HostedGame` methods and three fields — the simulation, the state it
  renders from, and its HUD — where `app.rs` used to carry the whole frame.
  `Loop<S>` is now a type alias for the engine's, so `run`, `start` and
  `with_shell` are free functions rather than inherent methods on it.

  Its menu vocabulary shrank to the part that was ever breakout's: `Launch`, on
  `LAUNCH_ID = FIRST_GAME_ID`. `MenuAction::{Resume, Fullscreen, DebugOverlay}`
  and the ids that carry them are the engine's, and `web.rs` lost its whole
  `WebLoop` impl — `crcbl::web` blanket-implements it for every engine loop,
  taking the name and the summary line from `HostedGame::NAME` and
  `HostedGame::log_summary`.

  **Nothing about the game changed**, and its own tests are the evidence: all 79
  pass unmodified except where they reached a field that is now behind an
  accessor, and the browser gate ran 27/27 checks against a real WebGPU device.
  `app.rs` lost 309 lines and `web.rs` 27, against 30 of `GameGpu` forwards in
  `gpu.rs`.

- **flappy**: hosted by `crcbl::engine::Loop` too, on the same shape as breakout
  — `Flappy` is seven `HostedGame` methods over the simulation, its render state
  and its HUD; `Flap` on `FLAP_ID = FIRST_GAME_ID` is all its menu vocabulary
  still declares; `web.rs` lost its `WebLoop` impl.

  It needed nothing the seam did not already have, which is the useful result:
  the bird's wing animation is stepped by `FrameInfo::ticks`, the field added
  for exactly this. Its own 86 tests pass and its browser gate ran 27/27.
  `app.rs` lost 288 lines and `web.rs` 28, against 30 of `GameGpu` forwards.

- **asteroids**: hosted by `crcbl::engine::Loop` as well, and it gained a fix on
  the way: **a refused fullscreen is now reported.** The sample never called
  `check_mode_request`, so a player on a tiling window manager pressed F11 and
  got no window change and no log line saying why; the engine's loop checks once
  a frame for every game it hosts.

  `Fire` on `FIRE_ID = FIRST_GAME_ID` is what its menu vocabulary still
  declares. `render_alpha` stays — this is the sample that interpolates
  rotations across a tick, and `FrameInfo::alpha` is where the number now comes
  from. `app.rs` lost 234 lines and `web.rs` 29; its 93 tests pass and its
  browser gate ran 27/27.

  The seam grew `Loop::{set_paused, gpu_mut}` for it: a test paused the loop by
  assignment, and its sprite read-back takes `&mut self`.
  - **sandbox**: the last conversion, and the one that measures the others.
    `Sandbox` is a struct with **no fields**: the sandbox has no simulation, no
    HUD and no score, and it still runs, pauses, opens a menu, goes fullscreen
    and reports a summary — all of that is the engine's now. Its `MenuAction` is
    `Infallible`, which makes `MenuAction::Game` uninhabited and is the type
    system agreeing that its three buttons are the loop's.

  It also stops declaring the six reserved keys for itself. `DEBUG_OVERLAY_KEY`
  and its five siblings were the engine's constants already, and a second
  declaration is how "the same key does the same thing in every sample" quietly
  stops being true.

  `app.rs` lost 379 lines and `menu.rs` 29; its 35 tests pass.

  `FrameInfo::tick_dt` and `HostedGame::tick` widened from `f32` to `f64`, which
  is what `FrameClock::tick_dt_secs` reports — the sandbox is the only game that
  reads it, and narrowing it was the engine deciding a precision on a game's
  behalf. `Loop::events` joins the accessors for the same reason the others did:
  a test read the field.

- **horde**: hosted by `crcbl::engine::Loop`, and the sample that stretched the
  seam. Its level-up panel is three upgrades the run's seed picked, so
  `HostedGame::menu_kind` now takes the loop's own `MenuSet` and a game may
  rebuild a panel before the kind it returns is shown. Its debug panel carries a
  section no other sample has, so `HostedGame::debug_sections` exists — empty by
  default, because "this game adds no section" is the honest answer for the
  other four. And it is the first game with **two** menu actions, `Restart` on
  `RESTART_ID` and `Choose(n)` on a reserved block above it.

  It also gains the refused-fullscreen report, for the same reason asteroids
  did. `app.rs` lost 205 lines and `web.rs` 32; its 124 tests pass and its
  browser gate ran 27/27.

  **The CPU frame report moved into the engine.** `Loop::finish` logs the clock
  it was driven from, the frame count, and mean/fps/best/worst — `apps/horde`
  wrote that itself and `--wall-clock` exists to make it mean something; every
  hosted game gets it now. The scene stats it used to carry are on horde's own
  `Summary` instead, so `main.rs` prints them natively and `log_summary` does in
  the browser.

- **crcbl** (`crcbl::engine`, `crcbl::web`): the sample loops' shared machinery
  moves into the engine, in four further slices.

  `open_window` logs the backend, aligns the shell's event clock with the
  engine's and creates the window, taking the caller's `WindowDesc` because a
  title and a size are the game's. `MAX_FRAME_STEP` joins it as an engine
  constant: the browser behaviour it guards against is the shell's.

  `PolledBoot`, with the `PolledGpu` trait, owns browser start-up — the pump,
  the configure/device state machine, the fix for a canvas resized while the
  device request is in flight, and the refusal to restart a boot that already
  finished or failed. It hands back `Booted` rather than a loop, because
  assembling one is the game's.

  `MenuPump` owns the menu's half of a pump batch: the three menu keys
  (`MENU_UP_KEY`, `MENU_DOWN_KEY` and `MENU_ACTIVATE_KEY`, now the engine's
  alongside the three reserved ones), the select/press/activate routing, and the
  held-key list. It answers with a `WidgetId`, leaving the mapping to a game's
  own action enum where it belongs.

  `crcbl::web` takes the browser entry point's shared half: the status codes — a
  wire format the JS shim switches on, so one definition is the only way they
  stay in step — the bounded log queue, and the whole `App` lifecycle behind the
  `WebLoop` and `WebPending` traits. It is deliberately not gated to `wasm32`,
  because gating it would put its tests on the one target the suite never runs.

  `run_ticks` is the fixed-step accumulator, with the rule that a **paused**
  frame still drains — the alternative banks the pause and spends it in one
  catch-up burst on the frame the player resumes. `FrameBudget` replaces the
  three fields every sample carried separately, because the reconfigure cap
  exists only so that a budget counting _presented_ frames stays reachable.
  `lose_focus` releases every held key before pausing, so a game does not resume
  believing a key is still down. `drive` is the native driver, behind a
  `GameLoop` trait that `crcbl::web::WebLoop` now requires — so the native and
  browser paths provably step the same loop.

  `PointerCapture` holds what the loop remembers about the pointer between
  frames — where it was left and whether its button is down — and resolves a
  batch into a `PointerInput`. `ModeRequest` holds the fullscreen request and
  whether the window system agreed, reporting what the window actually is rather
  than what was asked for.

  Measured: the four `app.rs` files lost 919 lines, and the four `web.rs` files
  went from 2642 to 1466. What the samples keep is what genuinely differs — each
  game's `assemble`, its `MenuAction` handler, its HUD, and the one log line
  reporting what a finished run was worth.

- **crcbl** (`crcbl::engine`): `LoopError<G>` replaces the error enum each
  sample wrote out for itself. The five loop failures — `NoWindowSystem`,
  `Shell`, `Configure`, `NeverPresented` and `Gpu` — belong to the loop however
  the game above them is spelled, and `G` names whatever the game itself
  refuses. A game with nothing of its own to refuse leaves it at the default
  `Infallible`, which makes the `Game` variant uninhabited and costs nothing.

  `BreakoutError`, `FlappyError`, `AsteroidsError`, `HordeError` and
  `SandboxError` are now aliases for it, so they keep their names and every
  `Err(FlappyError::Gpu(…))` still reads the same. `ShellError`,
  `ConfigureError` and `GpuError` still convert with `?`; a game error is
  wrapped by name, `.map_err(FlappyError::Game)`, because a blanket `From<G>`
  cannot coexist with the three concrete ones — `G` may itself be `ShellError`.

  Two messages change as a result. The sandbox's `NoWindowSystem` hint no longer
  names a roadmap phase for the missing Win32 and AppKit backends, since the
  engine has no business quoting one; it still says a platform may have no shell
  backend and still points at `--headless`. And its `NeverPresented` message
  loses a run of eighteen spaces that a missing line continuation had baked into
  the string literal.

- **samples**: `apps/{breakout,flappy,asteroids,horde}` drop eleven dependencies
  apiece and `apps/sandbox` drops its last one. `glam::` is `crcbl::math::` and
  `log::` is `crcbl::log::` at every call site — the same crates through the
  umbrella, so no version can drift and no two copies of a `Mat4` can meet.

- **crcbl** (`crcbl::engine`): the default present mode is now `Fifo` rather
  than `Mailbox`. A windowed native run vsyncs unless it asks not to, where it
  previously ran uncapped. The browser is unchanged: its swapchain already
  logged `Fifo` before this and logs it after, because the WebGPU surface does
  not offer `Mailbox` for the old preference to have found.

- **horde** (`apps/horde`): the engine's fourth game and its scale sample — the
  core loop. One arena, one player with WASD movement and an auto-aiming weapon,
  three enemy kinds that seek and push off each other, contact damage, hit
  points, death and restart. Native and headless; `--max-enemies` sets the
  ceiling on live enemies (default 1500). Drawn as untextured quads through the
  UI pass, which the art sub-slice replaces.

  Where the earlier samples ask what the engine can host, this one asks **what
  one tick costs per live body**, so the interesting part is the query pattern.
  Separation is one `PhysicsSystem::overlap_sphere` per enemy per tick, of
  radius `r_self + slack` — and the omission of the _neighbour's_ radius is
  exact rather than sloppy, because a shape-aware overlap of radius `R` returns
  everything within `R + r_b`, which is precisely the pair set separation wants.
  Contact damage is one more such query, at `PLAYER_RADIUS`, where every result
  is by construction a hit. Aiming is a third, at the weapon's range, instead of
  a scan of the enemy list. The weapon itself is segment CCD.

  Provisional numbers were taken here and **superseded by the scale sub-slice
  below**, which measures a fixture that fits inside the arena and which
  separates a spread crowd from a converged one. Both sets are in
  `docs/plan/sample/03-horde.md` with their conditions.

  Two divergences from asteroids are deliberate. **The gun fires after the bolt
  sweep**, because a projectile swept on the tick it was created is swept from a
  point one whole step behind the muzzle, through the thing that fired it —
  asteroids has the same order the other way round, and the same latent segment.
  **A wall clamp is not a teleport**: it moves a body by at most one tick of
  travel, so it is a refit rather than the remove-and-re-insert asteroids'
  screen wrap needs.

- **horde** (`apps/horde`): art and progression. `.crpix` sprites for the
  player, the three enemy kinds and the XP pickups, baked by a `build.rs` and
  drawn through `SpriteRenderer` with `SampleMode::Pixel`, replacing the
  untextured quads the core loop shipped with. XP gems drop where an enemy died
  and are collected by walking over them; banking a threshold opens a "pick 1 of
  3" level-up screen over the frozen field, from a fixed pool of six upgrades
  (`RAPID FIRE`, `HEAVY BOLTS`, `SWIFT BOOTS`, `LONG BARREL`, `VITALITY`,
  `MAGNET`). Pause, level-up and death menus over `crcbl_render`'s shared menu
  art, with the pointer, F11 and focus handling the other samples have.

  **Two sheets, and the split is a batching decision.** `SpriteRenderer` starts
  a batch whenever consecutive sprites name a different sheet, so the player,
  all three enemy kinds and the gems are one 34-texel frame size in one sheet:
  the whole field is a single batch **whatever order it is emitted in**, with no
  grouping pass over the crowd and no way for the batch count to grow with the
  horde. Asteroids has to emit its rocks largest-first to hold three batches;
  this cannot get it wrong. What it costs is the transparent margin round the
  two small kinds — a runner is 13 texels of art inside a 34-texel quad — and
  that is bounded by the screen rather than by the field.

  The scale is 20 texels a world unit, chosen from the runner: three enemy kinds
  have to be told apart at a glance in a crowd, which needs about thirteen
  texels across, and 13 / 0.64 units is 20.3. No scale makes all three enemy
  collider boxes a whole number of texels — the radii were picked for how the
  game plays, and it would take 50 texels a unit — so the shared frame is the
  largest one, which at 20 is exactly 34, and each silhouette is drawn to its
  own collider inside it.

  A level-up **freezes the field**, and the freeze is simulation state rather
  than the loop's pause: which upgrade a run took changes what the simulation
  does, so a seeded replay has to reproduce it, and the menu presses a real
  digit key into the action map rather than calling into the game. The freeze
  costs one pass on the tick it opens — a zero velocity written to the player,
  every enemy and every bolt — rather than a branch on the tick's hot path.

- **horde** (`apps/horde`): audio, the longest run, the browser demo, and the
  scale measurement the sample exists for. Five procedural spatial cues — the
  gun, an enemy coming apart, a gem banked, a level gained and the player's own
  end — with the listener **on the player**, which is the first sample whose
  listener moves. The longest run survived is kept in `~/.config/horde/best.bin`
  or the browser's Origin Private File System, in whole seconds so the record
  compares as the `m:ss` the HUD shows. The demo is live at
  `https://crcbl.kryptic.sh/demos/horde/` and the browser gate covers it at
  26/26, alongside the other three.

  **`crcbl-audio` has no voice limit, and this is the first sample that could
  not ignore it.** A kill is a cue and a gem is a cue against a fire cooldown
  whose floor is a twentieth of a second, so a late run raises about forty a
  second and each is a voice that lives until it runs out. The sample caps
  itself at sixteen, refuses the newest, and counts the refusals — and keeps
  counting the _cue_, because "did this happen" and "was there a speaker free"
  are different questions and only the first is what a test should be able to
  ask.

  Two flags carry the measurement, and both are in the shipped binary because
  the numbers have to be reproducible from a command line: **`--prefill N`**
  stages `N` enemies over the whole arena before the first frame (the spawner
  would take over ten minutes to reach the plan's target and nothing survives
  that long) and raises `--max-enemies` to fit them; **`--wall-clock`** drives a
  headless run from the real monotonic clock, so the debug panel's frame-timing
  module measures the frame instead of reporting the fixed step a headless clock
  hands it. The panel also gains this sample's own `scene` section — field,
  culled, drawn, batches — so the numbers the sample's argument rests on are
  readable in the running game.

  **The measurement, with its conditions in `docs/plan/sample/03-horde.md`.** On
  a Radeon RX 7900 XTX (radv), release, headless offscreen ring at 960 × 720,
  single-threaded:
  - **The render side is flat and the exit criterion is met.** CPU frame time
    0.096 ms on an empty field and on a field of a thousand, and 0.120 ms with
    ten thousand — nine thousand more enemies for 24 µs a frame, 0.14 % of a
    16.67 ms budget. With the driver taken out (`--backend null`) the game's own
    share is 0.005 ms to 0.033 ms. The `sprites` GPU pass goes 0.006 ms to 0.023
    ms.
  - **The batching claim holds.** Two draw calls at every count, and still two
    over ten thousand sprites with the whole field packed inside the view so
    that nothing is culled.
  - **The transparent margin is visible and does not matter.** The average enemy
    fills 31.5 % of its shared 34 × 34 quad, weighted by the mix the spawner
    deals, so about 12 µs of the sprite pass is margin at a full screen of the
    crowd — 0.07 % of the budget, against a grouping pass and an emission order
    to get wrong.
  - **The tick is what breaks, and it breaks on _density_ rather than on
    count.** Ten thousand enemies cost 14.66 ms a tick spread over the arena and
    84.09 ms once the crowd has converged on the player. Separation is one
    broadphase query per body and a query costs what its answer costs; a horde
    converges by construction. So the sample carries about ten thousand spread
    and about three thousand converged, and the plan's single figure was always
    going to be one or the other.

  **What that says about P7 and P8**, which is the reason the sample was built
  out of order in the first place: P8 (`crcbl-jobs`, the parallel schedule) is
  worth the whole of the gap — the steering pass is order-independent by
  construction and has no shared mutable state — and P7 (GPU culling, indirect
  draws, instance deltas) can return at most 0.7 % of a frame here, because the
  CPU cull it deletes costs 28 µs. The roadmap had horde waiting on P7; it was
  waiting on P8.

- **crcbl-render**: `Sprite::rotation` — sprites can turn. A per-sprite angle in
  radians, counter-clockwise, about the centre of the sprite's own `rect`. It
  rides in the fourth component of `SpriteInstance::sheet`, which was padding,
  so the instance is still 64 bytes and no buffer, stride or bind group changed.
  `Sprite` gains a field, so every struct literal that builds one needs
  `rotation: 0.0`; that is the only source-breaking part.

  Rotation interacts with `SampleMode::Pixel`, and both halves are decided
  rather than left to fall out. The **snap** stops rounding each corner once the
  quad is turned — a rotated quad has no axis-aligned rectangle to round onto,
  and rounding four corners independently shears it, changes its size and
  changes its effective angle, so a slowly turning ship would wobble — and
  instead translates the whole quad rigidly so its _centre_ lands on the pixel
  grid, which keeps the shape exact and still removes the sub-pixel crawl that
  translation causes. **Sharp bilinear needs no change at all**: `fwidth` is a
  per-fragment screen-space derivative, so it tracks the turned UV gradient by
  itself; being an L1 norm it reports up to root two times the scale on the
  diagonal, which widens the crossover band to about 1.4 fragments and never
  narrows it.

  A sprite with `rotation: 0.0` is **bit-identical** to one from before this
  change, by construction rather than by rounding luck: `sprite.slang` branches
  on the angle and the zero path is the arithmetic that was already there, down
  to the same SPIR-V `OpFMul`/`OpFAdd` pair. All eight existing golden images
  pass unchanged, at zero differing pixels.

- **crcbl-phys**: the broadphase BVH is **dynamic**. `Bvh::insert` and
  `Bvh::remove` add and drop one element along a single root-to-leaf path, and
  `PhysicsWorld::add_*` / `PhysicsWorld::remove` use them, so a world whose tree
  already exists no longer throws it away on every spawn and kill. A game that
  fires a bullet per shot and splits a rock into two used to pay a full
  `O(n log n)` rebuild for each of those events, every frame, on a tree it had
  just built. Batch population before the first query is unchanged: with no tree
  yet, adds accumulate and one bulk `Bvh::build` still runs, which produces a
  better tree than the same elements inserted one at a time.

  Insertion picks where a leaf goes by the surface area heuristic and the walk
  back to the root **rebalances** (AVL single rotation), which is what makes the
  quality claim hold rather than depend on the input. Measured over 20k
  insert/remove pairs: peak depth 13 at 1024 elements against an ideal of 11
  (`ceil(log2 n) + 1`), and 9 at 64 against 7. Without the rotation the same run
  on 1024 _coincident_ boxes — where every candidate site costs the same and the
  heuristic has nothing to choose by — reached depth 623, a tree that is very
  nearly a linked list. `Bvh::depth`, `Bvh::len` and `Bvh::is_empty` are public
  so the property is observable; `crates/crcbl-phys/tests/churn.rs` bounds depth
  by the AVL bound over thousands of operations and checks every query against a
  brute-force scan after each one.

- **crcbl-phys**: `ThrustForce` and `DampingForce`, the first two L1 force
  providers driven by a game rather than by physics for its own sake.

  `ThrustForce` is the first force that reads the body's _orientation_:
  `F = magnitude · (rotation × local_direction)`. The local axis is named rather
  than fixed at `Transform::forward` (`-Z`) because a top-down 2D game turns its
  ship about Z, where `-Z` points at the camera and thrusting along it would
  drive the ship out of the playfield plane. `ThrustForce::world_force` exposes
  the same vector to callers who are not using the provider pipeline.

  `DampingForce` is `F = -min(k, m/dt)·v`. The cap is the point: plain `-k·v`
  integrated at `k·dt/m ≥ 2` _reverses_ the velocity and then grows it, so a
  coefficient that behaves at a 240 Hz substep explodes at a 10 Hz one. With the
  cap the worst case is a velocity that reaches exactly zero. `DragForce` is
  deliberately left uncapped — it is the physical law, and a caller modelling a
  fluid wants the law.

- **crcbl-phys**: `PhysicsSystem::apply_force(entity, force)` adds a force to
  one entity for the next `step`. Force providers are global — every dynamic
  body gets every provider — which is right for gravity and wrong for the thrust
  of the one ship among a screenful of rocks.

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
- **asteroids**: a third game, playable headless and natively, and the
  workspace's first sample built around **entity churn** rather than around a
  fixed world. A ship that turns, thrusts and wraps; bullets that never miss;
  rocks in three sizes that split twice; waves that grow to a ceiling; score,
  three lives, game over and restart. Every random-looking number — where a wave
  enters, which way a split throws its children — is a pure function of a seed
  and an index, so a recorded script replays bit-identically and two games on
  one seed are the same game.

  It is the first consumer of the P6 physics slice, and the seams it uses are
  the ones that slice was bought for: `ThrustForce::world_force` through
  `PhysicsSystem::apply_force` for the engine, `sweep_sphere` over a
  `prev → cur` segment for every bullet, and `overlap_sphere` against the
  broadphase for the ship. **A wrap is a teleport, and a teleport is a
  remove-and-re-insert** — the rule `docs/backlog.md` left to whoever wrote the
  wrap, chosen here and applied uniformly to everything in the broadphase.

  It is drawn as **pixel art through the sprite pass**: five `.crpix` sheets
  under `apps/asteroids/assets/` — a ship, a shot, and one per rock size — baked
  to PNG by its own `build.rs` and drawn with `SampleMode::Pixel`. Ten texels to
  the world unit, chosen by the small rock: eleven texels is the least a rock
  can be and still have a lump stick out and a bite go in, and eleven over that
  rock's 1.1-unit diameter fixes the scale. Every rock's frame is then its
  collider's bounding square to the texel — 34, 20 and 11 — and the three are
  three drawings rather than one at three magnifications, which is what makes a
  split read as a rock breaking rather than as a rock shrinking.

  **It is also the first sample where a drawn thing turns**, which the
  `Sprite::rotation` above only made possible. The ship's heading and every
  rock's tumble are integrated once per simulation tick, so drawing the newest
  value on every frame stutters at any refresh rate that is not the tick rate;
  the renderer interpolates instead, with the frame clock's alpha.
  `game::lerp_angle` takes the **short way round**, which is the whole
  difficulty: a plain lerp from 350° to 10° spins the long way, once, on the
  frame after the heading crosses zero — and `turn_ship` keeps the heading in
  `[0, τ)`, so it crosses constantly. Positions are deliberately _not_
  interpolated: this playfield wraps, and unlike an angle a wrapped position is
  a real discontinuity.

  Presentation is the shape the other two samples set: start, pause and
  game-over menus through `crcbl_render::MenuRenderer`, Escape to pause, F11 for
  fullscreen, F3 for the debug panel, and a window that loses focus pausing and
  releasing every key it was holding. That last one matters more here than in
  either earlier sample, because turning and thrusting are _held_ actions: a
  release that never arrives is a ship that spins for the rest of the session.

  **Sound**: three spatial cues through `crcbl-audio`'s grammar — the engine,
  the gun, and a rock (or the ship) coming apart. The listener is the camera at
  the middle of the field and it never moves, so unlike in either earlier sample
  the pan and the distance both swing their full range: emitters are spread over
  the whole 32 × 24 playfield and cross it constantly. The explosion is a
  decaying burst of low-passed noise from a fixed seed rather than a tone,
  because a beep reads as scoring rather than as destruction. Thrust is the
  first _sustained_ cue any sample has needed and `crcbl-audio` has no looping
  voice, so it is a one-shot re-fired every `THRUST_CUE_PERIOD` — a constant
  that lives in the simulation, because the cue is raised inside the
  deterministic tick.

  **A best score**, kept in `~/.config/asteroids/best.bin` natively, in the
  Origin Private File System in a browser, and nowhere at all under
  `--headless`. Recorded once, on the edge into game over.

  **A browser build**: `apps/asteroids` is a `cdylib` on
  `wasm32-unknown-unknown` and the demo is live at
  `https://crcbl.kryptic.sh/demos/asteroids/`. `Loop` gained
  `PendingLoop`/`set_frame_step` and `Gpu` gained `request_open`, so start-up is
  polled across `requestAnimationFrame` frames instead of blocking on a promise
  the page's own event loop has to resolve. `web/run-browser-e2e.sh` drives it
  in a real Chromium for 26/26 checks, the same as the other two.

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
- **crcbl-ui**: `MenuSet<K>`, the container a game keeps its menus in. `Menu` is
  one panel; a game has several and needs to say which one a frame draws, to
  switch between them without carrying a half-finished click across, and to
  share one `UiState` so a press and its release are tested against the same
  capture. `K` is the game's own state type rather than one this crate dictates,
  and **a `K` the set holds no menu for draws nothing** — which is how "no menu
  this frame" is spelled, with no separate `Option`. `show`, `current`,
  `current_mut`, `is_showing`, `kind`, `select_next`, `select_previous`,
  `press`, `activate`, `point`, and `replace` for a panel whose buttons are
  built while the game runs. Both `show` and `replace` drop the pointer's
  capture; two entries claiming the same `K` are refused at construction,
  because the second would be unreachable.

### Changed

- **`crcbl-audio`**: the `Mixer` can now be driven by the game that owns it, and
  all four samples use it instead of a hand-rolled copy.

  `Mixer::play` took `&mut self` while `AudioStream::open` consumes its source,
  so once the stream was running nothing could reach the mixer to play through
  it — the shipped mixer was unreachable, and `apps/breakout`, `apps/flappy`,
  `apps/asteroids` and `apps/horde` had each written their own `Sound`, `Voice`,
  `VoiceQueue` and `MixerSource` around it. `play` now takes `&self` and answers
  with a `VoiceId`; `AudioSource` is implemented for `Arc<T>`, so
  `AudioStream::open(Arc::clone(&mixer))` leaves the game a handle to go on
  playing through. Existing callers keep compiling: no signature was narrowed,
  and `Mixer::play`'s new return value can be ignored.

  New alongside it: `Mixer::stop`, `Mixer::is_playing`, `Mixer::set_mix` and
  `Mixer::voice_mixes`; `VoiceId` and `VoiceMix`, with
  `VoiceMix::from(&SpatialCue)` as the "play this buffer once, panned" glue each
  sample was writing by hand (the cue's `itd_samples` is dropped — a `Voice` has
  no delay line); `Voice::with_mix`, `Voice::mix`, `Voice::is_looping` and
  `Voice::from_shared`; and `SoundBank::sound` / `SoundBank::insert_shared`.

  **`SoundBank::create_voice` no longer copies the sound.** `Voice` holds
  `Arc<[AudioSample]>`, so a voice is a playhead over the bank's buffer rather
  than a clone of it — at horde's cue rate that was an allocation the size of
  the sound per cue.

- **asteroids**: the engine is a real held sound, and an audio detail has left
  the simulation. `game::THRUST_CUE_PERIOD` and `GameLogic`'s `thrust_cue_timer`
  are **removed**: thrust used to be a one-shot re-fired on a countdown that
  lived in the deterministic tick, because the crate had no reachable looping
  voice. It is now one looping voice that `audio::Audio::set_thrust` starts on
  the first burning tick, re-aims at the ship every tick after (so the engine
  still pans across the field), and stops the tick the key comes up or the ship
  dies. What the simulation keeps is a plain `thrusting` bool, mirrored onto
  `Game::thrusting`.

  `THRUST_CUE_PERIOD` was re-exported from `apps/asteroids/src/lib.rs` and is
  gone from there too.

- **horde**: the game no longer starts itself. It opens on a `HORDE` start
  screen with a `PLAY` button — `Space`, which is the key breakout, flappy and
  asteroids print on theirs, and `R` still works — and the simulation does not
  advance until it is pressed: no spawns, no clock, no shots. The new
  `GameState::WaitingToStart` short-circuits the tick the way `LevelUp` already
  did, so a player looking at the title screen is looking at a still, empty
  arena rather than at a run that has been taking hit points off them since the
  window opened.

  **`TRY AGAIN` on the death screen now lands on that start screen**, not
  straight back into a run, which is what asteroids and flappy already do —
  restarting is two presses. `--prefill` starts its own run so the scale
  measurement still measures a running one. The sample deliberately shipped
  without a start screen; `docs/backlog.md` carries why that call was reversed.

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

- **crcbl-vk**: a readback whose explicit wait semaphore was destroyed between
  `request_readback` and `poll_readback` was undefined behaviour — the
  completion point was stored as the raw `VkSemaphore` and dereferenced at poll
  time with no liveness check. It is now stored as a generational handle and
  re-resolved through the device pool, exactly like the readback buffer, so a
  destroyed semaphore reports `InvalidHandle` instead.

- **crcbl-vk**: query commands with caller-supplied ranges no longer hand
  out-of-range values to the driver. `reset_query_set`, `write_timestamp` and
  `resolve_query_set` now bounds-check against the pool's query count at record
  time and fail with `InvalidDescriptor`, matching `Device::query_results` and
  the null backend — an over-large range used to be recorded and reached
  `vkCmdCopyQueryPoolResults`/`vkCmdResetQueryPool` as a validation violation.

- **crcbl-server**: a reconnect hello that arrived **after** the grace deadline
  expired the session without marking it terminated, so the next fresh join
  silently re-issued the dead session's token and id — and the departed client
  could still reconnect against the "new" session with its old credential. The
  expiry inside `handle_hello` now sets `session_terminated`, so the fresh join
  rotates to a new session and token.

- **crcbl-client**: a client holding a resume token a restarted server no longer
  recognised retried the stale token forever at capped backoff, wedged at
  "connecting" with no fresh join ever sent. Two consecutive
  `INVALID_SESSION_TOKEN` rejections now drop the token and session id and fall
  back to a fresh token-less join (two rather than one, so a single forged
  reject cannot throw away a still-valid credential).

- **asteroids**: a bullet could hit a rock sitting **behind** the ship on the
  tick it left the gun. Segment CCD reconstructs where a projectile was as
  `position - velocity * dt`, so one created this tick was swept from a point a
  whole step behind the muzzle — through the hull and out the other side. The
  gun fires after the sweep now, as `apps/horde` already did, so a bullet's
  first sweep is its first real step. 0.4 of a unit at 60 Hz and six units at
  `--tick-hz 4`, which is where the new test looks.

- **crcbl-vk**: reusing an image from the **offscreen ring** was ordered against
  nothing, so the frame that took the image back could write it while the
  previous frame was still reading it. A headless frame ends in
  `vkCmdCopyImageToBuffer` — a read — and the next frame opens with a layout
  transition out of `ResourceState::Undefined`, which is a write that discards
  the contents. `Undefined` maps to `srcStageMask = NONE`, which is right for a
  WSI image because the acquire semaphore already carries that dependency, and
  wrong for a ring image because there is no such semaphore: the seam hands one
  back with an implicit acquire. Nothing separated the two.

  The transition out of `Undefined` on a ring image now widens its source stage
  to `ALL_COMMANDS`, whose first synchronisation scope covers everything already
  submitted to the queue — the missing dependency, and nothing more: the access
  mask stays empty, because a write-after-read needs execution ordering and no
  cache flush, and the contents are still discarded. WSI images, ordinary
  images, and the seam's public shape are all unchanged, and no caller needs a
  change.

  Affects offscreen and headless Vulkan rendering that outlives the ring:
  `crcbl screenshot`, the `crcbl-vk` e2e suite, and `--headless --backend vk`.
  Windowed rendering is untouched. Validation reports the bug as
  `SYNC-HAZARD-WRITE-AFTER-READ` with `read_barriers: VkPipelineStageFlags2(0)`
  — that empty mask being precisely the `NONE` above; without a layer it is a
  race whose outcome the GPU's speed decides.

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
