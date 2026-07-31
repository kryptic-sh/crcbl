//! The Web/canvas shell backend (P5).
//!
//! Implements [`Shell`] for a browser `<canvas>` driven by a hand-rolled JS shim.
//! The shim calls into wasm via `extern "C"` functions; the wasm side returns
//! events by pushing them into a queue that [`pump`](WebShell::pump) drains.
//!
//! # Architecture
//!
//! `docs/plan/15-windowing.md` chooses a JS shim over `wasm-bindgen` / `web-sys`.
//! Events enter the wasm side through `extern "C"` functions the JS shim calls:
//! `__crcbl_web_resize`, `__crcbl_web_key`, `__crcbl_web_pointer`, etc.
//! Each pushes a [`ShellEvent`] into a global queue.
//!
//! # Capabilities
//!
//! The web backend reports [`ShellCaps::DESKTOP`] minus what it cannot do:
//! * No [`MULTI_WINDOW`](crate::ShellCaps::MULTI_WINDOW)
//! * No [`POINTER_WARP`](crate::ShellCaps::POINTER_WARP)
//! * No [`CLIPBOARD`](crate::ShellCaps::CLIPBOARD)
//! * No [`DRAG_DROP`](crate::ShellCaps::DRAG_DROP)
//! * No [`EVENT_WAIT`](crate::ShellCaps::EVENT_WAIT)
//!
//! # The configure handshake
//!
//! A window created here has no size until the JS side reports one via
//! `__crcbl_web_resize`. The first resize provides the canvas's layout size;
//! subsequent ones come from `ResizeObserver`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use core::time::Duration;

use crcbl_core::{
    EventTime, KeyCode, SurfaceTarget,
    input::{ButtonState, DeviceId, Keysym, Modifiers, PointerButton, ScrollDelta},
};

use crate::{
    ClipboardOffer, ClipboardRequestId, CloseReply, CursorIcon, DisplayMode, MonitorId,
    MonitorInfo, PhysicalPoint, PhysicalRect, PhysicalSize, PointerMode, Shell, ShellBackend,
    ShellCaps, ShellError, ShellEvent, SizeConstraints, WindowConfiguration, WindowDesc, WindowId,
    WindowState,
};

// ---------------------------------------------------------------------------
// Global event queue — the bridge between JS→wasm calls and pump()
// ---------------------------------------------------------------------------

thread_local! {
    static WEB_EVENTS: RefCell<VecDeque<ShellEvent>> = const { RefCell::new(VecDeque::new()) };
    static WEB_TIME_MS: RefCell<f64> = const { RefCell::new(0.0) };
}

/// Push an event from JS. Called by `extern "C"` functions below.
fn push_event(event: ShellEvent) {
    WEB_EVENTS.with(|queue| queue.borrow_mut().push_back(event));
}

/// The browser's `performance.now()` in ms, set by the rAF callback.
fn now_ms() -> f64 {
    WEB_TIME_MS.with(|t| *t.borrow())
}

/// Convert the browser's monotonic ms to [`EventTime`].
fn event_time(offset_ms: f64) -> EventTime {
    EventTime::from_millis(offset_ms as u64)
}

/// Build a window handle from a canvas id, using a fixed generation so the
/// extern "C" entry points can reconstruct it from a u32 index.
fn web_window_handle(canvas_index: u32) -> WindowId {
    // Pack: generation=1 in upper 32, index in lower 32.
    WindowId::from_bits(0x0000_0001_0000_0000 | canvas_index as u64).expect("valid window handle")
}

// ---------------------------------------------------------------------------
// extern "C" entry points — called from JS
// ---------------------------------------------------------------------------

/// Called by the JS shim each rAF with the current timestamp.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __crcbl_web_frame(now_ms: f64) {
    WEB_TIME_MS.with(|t| *t.borrow_mut() = now_ms);
}

/// Canvas size changed (from ResizeObserver or initial layout).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __crcbl_web_resize(
    window_index: u32,
    width: u32,
    height: u32,
    scale: f64,
) {
    push_event(ShellEvent::Resized {
        window: web_window_handle(window_index),
        size: PhysicalSize::new(width, height),
        scale_factor: scale.max(1.0),
    });
}

/// Keyboard event from the canvas.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __crcbl_web_key(
    window_index: u32,
    key_code: u32,
    pressed: u32,
    repeat: u32,
    ctrl: u32,
    shift: u32,
    alt: u32,
    meta: u32,
) {
    let time = now_ms();
    let mut modifiers = Modifiers::empty();
    if ctrl != 0 {
        modifiers |= Modifiers::CTRL;
    }
    if shift != 0 {
        modifiers |= Modifiers::SHIFT;
    }
    if alt != 0 {
        modifiers |= Modifiers::ALT;
    }
    if meta != 0 {
        modifiers |= Modifiers::SUPER;
    }

    let key = js_key_code_to_crucible(key_code);

    push_event(ShellEvent::Key {
        window: web_window_handle(window_index),
        device: DeviceId::UNKNOWN,
        time: event_time(time),
        scancode: crate::Scancode(key_code),
        key_code: key,
        keysym: Keysym::NONE,
        state: if pressed != 0 {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        },
        repeat: repeat != 0,
        modifiers,
    });
}

/// Pointer (mouse/touch) event from the canvas.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __crcbl_web_pointer(
    window_index: u32,
    kind: u32, // 0=motion, 1=down, 2=up, 3=wheel, 4=enter, 5=leave
    x: f64,
    y: f64,
    button: u32,
    delta_x: f64,
    delta_y: f64,
) {
    let time = now_ms();
    let window = web_window_handle(window_index);
    let position = PhysicalPoint::new(x, y);

    match kind {
        0 => {
            push_event(ShellEvent::PointerMotion {
                window,
                device: DeviceId::UNKNOWN,
                time: event_time(time),
                abs: Some(position),
                raw_delta: None,
            });
        }
        1 | 2 => {
            let btn = match button {
                0 => PointerButton::Left,
                1 => PointerButton::Middle,
                2 => PointerButton::Right,
                n => PointerButton::Other(n as u16),
            };
            push_event(ShellEvent::Button {
                window,
                device: DeviceId::UNKNOWN,
                time: event_time(time),
                button: btn,
                state: if kind == 1 {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                },
                position: Some(position),
                modifiers: Modifiers::empty(),
            });
        }
        3 => {
            push_event(ShellEvent::Wheel {
                window,
                device: DeviceId::UNKNOWN,
                time: event_time(time),
                delta: ScrollDelta::Pixels {
                    x: delta_x,
                    y: delta_y,
                },
                position: Some(position),
                modifiers: Modifiers::empty(),
            });
        }
        4 | 5 => {
            push_event(ShellEvent::PointerFocus {
                window,
                device: DeviceId::UNKNOWN,
                time: event_time(time),
                entered: kind == 4,
                position: if kind == 4 { Some(position) } else { None },
            });
        }
        _ => {}
    }
}

/// Map JS `event.keyCode` to our [`KeyCode`].
///
/// Returns `None` for unrecognized codes. A real keymap would be larger;
/// P6+ can improve this with the `code` field from the KeyboardEvent.
fn js_key_code_to_crucible(code: u32) -> Option<KeyCode> {
    match code {
        // Letters
        65 => Some(KeyCode::KeyA),
        66 => Some(KeyCode::KeyB),
        67 => Some(KeyCode::KeyC),
        68 => Some(KeyCode::KeyD),
        69 => Some(KeyCode::KeyE),
        70 => Some(KeyCode::KeyF),
        71 => Some(KeyCode::KeyG),
        72 => Some(KeyCode::KeyH),
        73 => Some(KeyCode::KeyI),
        74 => Some(KeyCode::KeyJ),
        75 => Some(KeyCode::KeyK),
        76 => Some(KeyCode::KeyL),
        77 => Some(KeyCode::KeyM),
        78 => Some(KeyCode::KeyN),
        79 => Some(KeyCode::KeyO),
        80 => Some(KeyCode::KeyP),
        81 => Some(KeyCode::KeyQ),
        82 => Some(KeyCode::KeyR),
        83 => Some(KeyCode::KeyS),
        84 => Some(KeyCode::KeyT),
        85 => Some(KeyCode::KeyU),
        86 => Some(KeyCode::KeyV),
        87 => Some(KeyCode::KeyW),
        88 => Some(KeyCode::KeyX),
        89 => Some(KeyCode::KeyY),
        90 => Some(KeyCode::KeyZ),
        // Digits
        48 => Some(KeyCode::Digit0),
        49 => Some(KeyCode::Digit1),
        50 => Some(KeyCode::Digit2),
        51 => Some(KeyCode::Digit3),
        52 => Some(KeyCode::Digit4),
        53 => Some(KeyCode::Digit5),
        54 => Some(KeyCode::Digit6),
        55 => Some(KeyCode::Digit7),
        56 => Some(KeyCode::Digit8),
        57 => Some(KeyCode::Digit9),
        // Navigation
        37 => Some(KeyCode::ArrowLeft),
        38 => Some(KeyCode::ArrowUp),
        39 => Some(KeyCode::ArrowRight),
        40 => Some(KeyCode::ArrowDown),
        // Modifiers
        16 => Some(KeyCode::ShiftLeft),
        17 => Some(KeyCode::ControlLeft),
        18 => Some(KeyCode::AltLeft),
        91 => Some(KeyCode::SuperLeft),
        // Special
        32 => Some(KeyCode::Space),
        13 => Some(KeyCode::Enter),
        27 => Some(KeyCode::Escape),
        9 => Some(KeyCode::Tab),
        8 => Some(KeyCode::Backspace),
        46 => Some(KeyCode::Delete),
        36 => Some(KeyCode::Home),
        35 => Some(KeyCode::End),
        33 => Some(KeyCode::PageUp),
        34 => Some(KeyCode::PageDown),
        // Function keys
        112 => Some(KeyCode::F1),
        113 => Some(KeyCode::F2),
        114 => Some(KeyCode::F3),
        115 => Some(KeyCode::F4),
        116 => Some(KeyCode::F5),
        117 => Some(KeyCode::F6),
        118 => Some(KeyCode::F7),
        119 => Some(KeyCode::F8),
        120 => Some(KeyCode::F9),
        121 => Some(KeyCode::F10),
        122 => Some(KeyCode::F11),
        123 => Some(KeyCode::F12),
        // Numeric keypad
        96 => Some(KeyCode::Numpad0),
        97 => Some(KeyCode::Numpad1),
        98 => Some(KeyCode::Numpad2),
        99 => Some(KeyCode::Numpad3),
        100 => Some(KeyCode::Numpad4),
        101 => Some(KeyCode::Numpad5),
        102 => Some(KeyCode::Numpad6),
        103 => Some(KeyCode::Numpad7),
        104 => Some(KeyCode::Numpad8),
        105 => Some(KeyCode::Numpad9),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// WebShell — the backend
// ---------------------------------------------------------------------------

/// The web backend: a single `<canvas>` element, event queue, and a virtual
/// monitor sized to the canvas.
#[derive(Debug)]
pub struct WebShell {
    canvas_id: u32,
    window: WindowId,
    monitors: Vec<MonitorInfo>,
    configured: Option<WindowConfiguration>,
    focused: bool,
    pointer_mode: PointerMode,
    cursor: Option<CursorIcon>,
    close_pending: bool,
    events: EventQueue,
}

type EventQueue = Rc<RefCell<VecDeque<ShellEvent>>>;

impl WebShell {
    /// Creates a web shell wrapping the canvas registered as `canvas_id`.
    #[must_use]
    pub fn new(canvas_id: u32) -> Self {
        let window = web_window_handle(canvas_id);
        let monitors = vec![MonitorInfo {
            id: MonitorId(1),
            name: "canvas".to_string(),
            bounds: PhysicalRect::new(0, 0, 1280, 720),
            work_area: PhysicalRect::new(0, 0, 1280, 720),
            scale_factor: 1.0,
            refresh_millihertz: 60_000,
            is_primary: true,
        }];

        let events: EventQueue = Rc::new(RefCell::new(VecDeque::new()));

        // Wire the global queue to forward into this shell's event queue.
        QUEUE.with(|q| {
            *q.borrow_mut() = Some(events.clone());
        });

        Self {
            canvas_id,
            window,
            monitors,
            configured: None,
            focused: true,
            pointer_mode: PointerMode::Free,
            cursor: Some(CursorIcon::Default),
            close_pending: false,
            events,
        }
    }

    fn configure(&mut self, size: PhysicalSize, scale_factor: f64) {
        self.configured = Some(WindowConfiguration {
            size,
            scale_factor,
            mode: DisplayMode::Borderless { monitor: None },
        });
        if let Some(monitor) = self.monitors.first_mut() {
            monitor.bounds = PhysicalRect::new(0, 0, size.width, size.height);
            monitor.work_area = PhysicalRect::new(0, 0, size.width, size.height);
            monitor.scale_factor = scale_factor;
        }
    }
}

impl Shell for WebShell {
    fn backend(&self) -> ShellBackend {
        ShellBackend::Web
    }

    fn caps(&self) -> ShellCaps {
        ShellCaps::DESKTOP
            & !ShellCaps::MULTI_WINDOW
            & !ShellCaps::POINTER_WARP
            & !ShellCaps::CLIPBOARD
            & !ShellCaps::DRAG_DROP
            & !ShellCaps::EVENT_WAIT
    }

    fn create_window(&mut self, _desc: &WindowDesc<'_>) -> Result<WindowId, ShellError> {
        Ok(self.window)
    }

    fn destroy_window(&mut self, window: WindowId) -> Result<(), ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        self.events
            .borrow_mut()
            .push_back(ShellEvent::WindowDestroyed { window });
        Ok(())
    }

    fn window_state(&self, window: WindowId) -> Result<WindowState, ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        Ok(WindowState {
            configuration: self.configured,
            requested_mode: DisplayMode::Borderless { monitor: None },
            requested_constraints: SizeConstraints::NONE,
            focused: self.focused,
            visible: true,
            pointer_mode: self.pointer_mode,
            close_pending: self.close_pending,
        })
    }

    fn set_title(&mut self, _window: WindowId, _title: &str) -> Result<(), ShellError> {
        Ok(())
    }

    fn set_visible(&mut self, _window: WindowId, _visible: bool) -> Result<(), ShellError> {
        Ok(())
    }

    fn set_mode(&mut self, _window: WindowId, _mode: DisplayMode) -> Result<(), ShellError> {
        Ok(())
    }

    fn set_constraints(
        &mut self,
        _window: WindowId,
        _constraints: SizeConstraints,
    ) -> Result<(), ShellError> {
        Ok(())
    }

    fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    fn pump(&mut self, sink: &mut dyn FnMut(ShellEvent)) {
        // Drain events without holding the borrow across self.configure().
        let events: Vec<ShellEvent> = self.events.borrow_mut().drain(..).collect();
        for event in events {
            if let ShellEvent::Resized {
                size, scale_factor, ..
            } = &event
            {
                self.configure(*size, *scale_factor);
            }
            sink(event);
        }
    }

    fn wait_events(&mut self, _timeout: Option<Duration>) {}

    fn surface_target(&self, window: WindowId) -> Result<SurfaceTarget, ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        Ok(SurfaceTarget::Web {
            canvas_id: self.canvas_id,
        })
    }

    fn set_pointer_mode(&mut self, window: WindowId, mode: PointerMode) -> Result<(), ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        let required = mode.required_cap();
        if !self.caps().contains(required) {
            return Err(ShellError::Unsupported {
                backend: ShellBackend::Web,
                what: match mode {
                    PointerMode::Confined => "pointer confinement",
                    PointerMode::Locked => "pointer lock",
                    PointerMode::Free => unreachable!(),
                },
            });
        }
        self.pointer_mode = mode;
        Ok(())
    }

    fn set_cursor(
        &mut self,
        window: WindowId,
        cursor: Option<CursorIcon>,
    ) -> Result<(), ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        self.cursor = cursor;
        Ok(())
    }

    fn warp_pointer(
        &mut self,
        window: WindowId,
        _position: PhysicalPoint,
    ) -> Result<(), ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        Err(ShellError::Unsupported {
            backend: ShellBackend::Web,
            what: "pointer warp",
        })
    }

    fn reply_close_request(
        &mut self,
        window: WindowId,
        reply: CloseReply,
    ) -> Result<(), ShellError> {
        if window != self.window {
            return Err(ShellError::invalid_window(window));
        }
        if !self.close_pending {
            return Err(ShellError::NoPendingCloseRequest { window });
        }
        self.close_pending = false;
        match reply {
            CloseReply::Close => Ok(()),
            CloseReply::Keep => Ok(()),
        }
    }

    fn clipboard_offer(
        &mut self,
        _window: WindowId,
        _offers: &[ClipboardOffer<'_>],
    ) -> Result<(), ShellError> {
        Err(ShellError::Unsupported {
            backend: ShellBackend::Web,
            what: "clipboard",
        })
    }

    fn clipboard_request(
        &mut self,
        _window: WindowId,
        _mime: crate::MimeType,
    ) -> Result<ClipboardRequestId, ShellError> {
        Err(ShellError::Unsupported {
            backend: ShellBackend::Web,
            what: "clipboard",
        })
    }

    fn clipboard_readable(&self, _window: WindowId) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Thread-local queue bridge (P6+: wire to the active shell)
// ---------------------------------------------------------------------------

thread_local! {
    pub(crate) static QUEUE: RefCell<Option<EventQueue>> =
        const { RefCell::new(None) };
}

/// Creates a web shell for the given canvas id. Called from the backend
/// registry when `CRCBL_SHELL=web` or `open_backend(Web)`.
pub fn open(canvas_id: u32) -> Result<WebShell, ShellError> {
    Ok(WebShell::new(canvas_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_shell_creates_with_no_configured_size() {
        let shell = WebShell::new(7);
        assert_eq!(shell.backend(), ShellBackend::Web);
        let state = shell.window_state(shell.window).expect("valid window");
        assert!(!state.is_configured());
        assert_eq!(state.size(), None);
    }

    #[test]
    fn surface_target_is_web_variant() {
        let shell = WebShell::new(3);
        let target = shell.surface_target(shell.window).expect("valid window");
        assert_eq!(target, SurfaceTarget::Web { canvas_id: 3 });
    }

    #[test]
    fn caps_exclude_platform_unsupported() {
        let shell = WebShell::new(0);
        let caps = shell.caps();
        assert!(!caps.contains(ShellCaps::MULTI_WINDOW));
        assert!(!caps.contains(ShellCaps::POINTER_WARP));
        assert!(!caps.contains(ShellCaps::CLIPBOARD));
        assert!(!caps.contains(ShellCaps::EVENT_WAIT));
        assert!(caps.contains(ShellCaps::RAW_POINTER_MOTION));
    }

    #[test]
    fn resize_event_updates_window_state() {
        let mut shell = WebShell::new(0);
        shell.events.borrow_mut().push_back(ShellEvent::Resized {
            window: shell.window,
            size: PhysicalSize::new(1920, 1080),
            scale_factor: 2.0,
        });
        let mut events = Vec::new();
        shell.pump(&mut |e| events.push(e));
        assert_eq!(events.len(), 1);
        let state = shell.window_state(shell.window).expect("valid window");
        assert_eq!(state.size(), Some(PhysicalSize::new(1920, 1080)));
        assert_eq!(state.scale_factor(), Some(2.0));
    }

    #[test]
    fn warp_pointer_is_unsupported() {
        let mut shell = WebShell::new(0);
        assert!(
            shell
                .warp_pointer(shell.window, PhysicalPoint::new(100.0, 200.0))
                .is_err()
        );
    }

    #[test]
    fn clipboard_is_unsupported() {
        let mut shell = WebShell::new(0);
        assert!(shell.clipboard_offer(shell.window, &[]).is_err());
        assert!(
            shell
                .clipboard_request(shell.window, crate::MimeType::TextUtf8)
                .is_err()
        );
        assert!(!shell.clipboard_readable(shell.window));
    }

    #[test]
    fn stale_handles_are_rejected() {
        let shell = WebShell::new(0);
        let foreign = WindowId::from_bits(0xDEAD_BEEF_0001_0001).expect("valid bits");
        assert!(shell.window_state(foreign).is_err());
        assert!(shell.surface_target(foreign).is_err());
    }

    #[test]
    fn js_key_code_to_crucible_smoke() {
        assert_eq!(js_key_code_to_crucible(65), Some(KeyCode::KeyA));
        assert_eq!(js_key_code_to_crucible(32), Some(KeyCode::Space));
        assert_eq!(js_key_code_to_crucible(37), Some(KeyCode::ArrowLeft));
        assert_eq!(js_key_code_to_crucible(112), Some(KeyCode::F1));
        assert_eq!(js_key_code_to_crucible(0), None);
        assert_eq!(js_key_code_to_crucible(999), None);
    }
}
