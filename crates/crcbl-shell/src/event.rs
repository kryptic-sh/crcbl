//! [`ShellEvent`] — everything the window system tells the engine, in engine
//! types.
//!
//! # Timestamps, and the clock they are on
//!
//! Every input-bearing variant carries an [`EventTime`]. This is not
//! decoration: `docs/plan/19-input.md` makes the pattern evaluator "a pure
//! function over timestamped edges" — tap versus hold versus double-tap are
//! *defined* by inter-event durations — and topic 26's client prediction needs
//! input-to-photon latency, which is a subtraction between an event timestamp
//! and a present timestamp. Neither works if the timestamp is the moment the
//! engine got around to draining the queue: a 20 ms frame quantizes every
//! duration to 20 ms and makes a fast double-tap indistinguishable from a slow
//! one.
//!
//! So the timestamp is the **window system's**, rebased onto the engine's
//! monotonic clock. [`EventTime`]'s own documentation states the epoch and
//! spells out the per-platform rebasing each backend owes; the short version is
//! that an `EventTime` and [`FrameClock`](crcbl_core::FrameClock) readings live
//! on one timeline, so they can be subtracted.
//!
//! Non-input variants ([`Resized`](ShellEvent::Resized),
//! [`MonitorsChanged`](ShellEvent::MonitorsChanged)) carry no timestamp. They
//! are state transitions, not samples; nothing computes a duration between two
//! resizes, and a field that exists only to be ignored invites someone to use
//! it for something it is not.
//!
//! # Modifiers are stamped onto every event, not delivered separately
//!
//! Wayland sends `wl_keyboard.modifiers` as its own message, X11 puts a
//! modifier mask on each event, and Win32 makes you call `GetKeyState`. The
//! seam picks the X11 shape: the backend tracks modifier state and stamps the
//! current value onto each event.
//!
//! The reason is that the alternative loses information the consumer cannot
//! recover. If modifiers arrive as a separate event, a consumer that batches a
//! frame's events — which every consumer does — sees "Ctrl went down" and "S
//! went down" as two facts in a list and has to replay them in order to know
//! whether it was `Ctrl+S`. That replay is exactly the state machine each
//! backend has to write anyway, so it is written once, in the backend, where
//! the ordering is authoritative.
//!
//! # Non-exhaustive, unlike [`SurfaceTarget`](crcbl_core::SurfaceTarget)
//!
//! `SurfaceTarget` is deliberately exhaustive so a new platform breaks every
//! HAL backend loudly. `ShellEvent` is the opposite: a consumer that ignores an
//! event it has never heard of degrades gracefully (touch input does nothing,
//! rather than doing something wrong), and gamepad hotplug and IME pre-edit are
//! both scheduled to land later. Breaking every `match` in the engine for each
//! is not a useful forcing function, it is churn.
//!
//! [`Touch`](ShellEvent::Touch) is what that promise looked like when it was
//! collected: it landed without touching a single `match` outside the shell,
//! and a game that never heard of it kept playing with one finger because the
//! primary contact is reported through the pointer variants as well.

use std::path::PathBuf;

use crcbl_core::EventTime;
use crcbl_core::input::{
    ButtonState, ContactId, DeviceId, Keysym, Modifiers, PointerButton, Scancode, ScrollDelta,
    TouchPhase,
};

use crate::{
    ClipboardContent, ClipboardRequestId, KeyCode, PhysicalPoint, PhysicalSize, ReceivedMime,
    WindowId,
};

/// Something the window system reported.
///
/// Delivered by [`Shell::pump`](crate::Shell::pump). See the [module
/// docs](self) for the timestamp and modifier conventions.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ShellEvent {
    /// The client area changed size.
    ///
    /// The authoritative size for the swapchain — never the size that was
    /// asked for in [`WindowDesc`](crate::WindowDesc). Carries the scale factor
    /// too, because a resize and a scale change can arrive together (dragging a
    /// window between monitors of different DPI does exactly that) and a
    /// consumer that reads size from one event and scale from another will
    /// briefly hold a mismatched pair.
    Resized {
        /// Which window.
        window: WindowId,
        /// New client area in device pixels.
        size: PhysicalSize,
        /// Device pixels per logical unit, at the same instant.
        scale_factor: f64,
    },

    /// The window's scale factor changed — it moved to a different monitor, or
    /// the user changed the display scale.
    ///
    /// Carries the *new* physical size as well: on Wayland a scale change is a
    /// `configure` that also restates the size, and on Win32 `WM_DPICHANGED`
    /// hands over a suggested rectangle. A consumer must treat this as a resize
    /// too. `docs/plan/15-windowing.md` calls out "scale-factor changes
    /// mid-session must not leak wrong-size swapchains" as a required test, and
    /// putting both facts in one event is what makes that testable rather than
    /// order-dependent.
    ScaleFactorChanged {
        /// Which window.
        window: WindowId,
        /// New device pixels per logical unit.
        scale_factor: f64,
        /// Client area in device pixels after the change.
        size: PhysicalSize,
    },

    /// The user asked to close the window — title-bar button, Alt-F4, session
    /// logout.
    ///
    /// The window is **still open**. Answer with
    /// [`reply_close_request`](crate::Shell::reply_close_request); until you
    /// do, the shell holds the request. See
    /// [`CloseReply`](crate::CloseReply).
    CloseRequested {
        /// Which window.
        window: WindowId,
    },

    /// The window was destroyed and its [`WindowId`] is now stale.
    ///
    /// Follows a [`CloseReply::Close`](crate::CloseReply::Close), an explicit
    /// [`destroy_window`](crate::Shell::destroy_window), or the compositor
    /// taking the window away.
    WindowDestroyed {
        /// The window that is gone.
        window: WindowId,
    },

    /// Keyboard focus entered or left the window.
    ///
    /// A consumer **must** treat focus loss as "every key is now up": no
    /// platform delivers releases for keys held when focus leaves, and a game
    /// that does not clear its key state walks into a wall until the player
    /// taps the key again. The action layer (P2) does this centrally; raw
    /// consumers do it themselves.
    Focus {
        /// Which window.
        window: WindowId,
        /// Whether the window now has focus.
        focused: bool,
    },

    /// A key went down or came up.
    ///
    /// Three views of the same keystroke — see
    /// [`crcbl_core::input`] for why they are all here and what each is for.
    Key {
        /// Which window had focus.
        window: WindowId,
        /// Which keyboard.
        device: DeviceId,
        /// When it happened; see the [module docs](self).
        time: EventTime,
        /// The platform's raw code, for keys the engine has no name for.
        scancode: Scancode,
        /// The physical key, layout-independent. `None` when the scancode maps
        /// to no key this engine names — the key is still bindable through
        /// `scancode`.
        key_code: Option<KeyCode>,
        /// The symbol this key produces in the current layout, for rebind
        /// menus and shortcut labels.
        keysym: Keysym,
        /// Down or up.
        state: ButtonState,
        /// Whether this is an auto-repeat, not a fresh press.
        ///
        /// Text fields want repeats; a jump button must not. Filtering them at
        /// the source would break the first, keeping them silently breaks the
        /// second, so the fact is carried and the consumer decides.
        repeat: bool,
        /// Modifiers held at the time; see the [module docs](self).
        modifiers: Modifiers,
    },

    /// The pointer moved.
    ///
    /// Both fields are optional, and which are present is the difference
    /// between a UI event and a camera event:
    ///
    /// * `abs` is where the pointer is, in window pixels. `None` under
    ///   [`PointerMode::Locked`](crate::PointerMode::Locked), where there is no
    ///   meaningful position.
    /// * `raw_delta` is unaccelerated, unclipped motion. `None` when the
    ///   backend has no relative-motion source — see
    ///   [`ShellCaps::RAW_POINTER_MOTION`](crate::ShellCaps::RAW_POINTER_MOTION).
    ///
    /// A camera must use `raw_delta` and must not difference successive `abs`
    /// values: the latter is clamped at the screen edge and has pointer
    /// acceleration already applied, which is why aim in games that get this
    /// wrong stops working when the cursor reaches the edge of the display.
    PointerMotion {
        /// Which window.
        window: WindowId,
        /// Which pointing device.
        device: DeviceId,
        /// When it happened.
        time: EventTime,
        /// Position in the window, in device pixels.
        abs: Option<PhysicalPoint>,
        /// Unaccelerated relative motion in device pixels, `(dx, dy)`.
        raw_delta: Option<(f64, f64)>,
    },

    /// The pointer entered or left the window.
    ///
    /// Not in `15-windowing.md`'s variant list, and added anyway: every backend
    /// delivers enter/leave natively (`wl_pointer.enter`, X11
    /// `EnterNotify`, `WM_MOUSELEAVE`), the UI layer needs it for hover state,
    /// and the alternative — inferring it from motion events that stop
    /// arriving — cannot distinguish "the pointer left" from "the pointer
    /// stopped moving".
    PointerFocus {
        /// Which window.
        window: WindowId,
        /// Which pointing device.
        device: DeviceId,
        /// When it happened.
        time: EventTime,
        /// Whether the pointer is now inside the window.
        entered: bool,
        /// Where it entered, if the backend said. Always `None` on leave.
        position: Option<PhysicalPoint>,
    },

    /// A pointer button went down or came up.
    Button {
        /// Which window.
        window: WindowId,
        /// Which pointing device.
        device: DeviceId,
        /// When it happened.
        time: EventTime,
        /// Which button.
        button: PointerButton,
        /// Down or up.
        state: ButtonState,
        /// Where the pointer was, if known. `None` under
        /// [`PointerMode::Locked`](crate::PointerMode::Locked).
        position: Option<PhysicalPoint>,
        /// Modifiers held at the time.
        modifiers: Modifiers,
    },

    /// A scroll wheel or touchpad scrolled.
    Wheel {
        /// Which window.
        window: WindowId,
        /// Which pointing device.
        device: DeviceId,
        /// When it happened.
        time: EventTime,
        /// How far, in the units the device reported — see
        /// [`ScrollDelta`], which does not collapse detents into pixels.
        delta: ScrollDelta,
        /// Where the pointer was, if known.
        position: Option<PhysicalPoint>,
        /// Modifiers held at the time. Ctrl+wheel is zoom nearly everywhere,
        /// so this is load-bearing rather than informational.
        modifiers: Modifiers,
    },

    /// A finger landed, moved, lifted, or had its gesture taken away.
    ///
    /// **One event per contact**, so two fingers moving at once are two events
    /// and not one averaged pointer. [`ContactId`] carries the identity and
    /// [`TouchPhase`] the lifecycle; between them a consumer can hold per-finger
    /// state, which is the whole reason this variant exists and the reason a
    /// second finger used to be dropped at the backend.
    ///
    /// # A touchscreen also produces pointer events, and that is deliberate
    ///
    /// The **primary** contact — the first finger of a gesture, and the only one
    /// while there is only one — is *also* reported as
    /// [`PointerMotion`](ShellEvent::PointerMotion) and
    /// [`Button`](ShellEvent::Button), exactly as a browser emulates mouse
    /// events for touch and Windows synthesizes them from `WM_POINTER`. So a
    /// game bound to `Binding::MouseButton` is playable with one finger without
    /// knowing this variant exists, and a game that wants two fingers reads
    /// `Touch` and ignores the emulation.
    ///
    /// That is an obligation on a backend that sets
    /// [`ShellCaps::TOUCH`](crate::ShellCaps::TOUCH), not an accident of one
    /// platform: a backend whose window system does not emulate must do it
    /// itself, or every existing pointer binding stops working the day it
    /// reports its first contact. Contacts that are not primary have no pointer
    /// events at all — a second finger is not the first one teleporting.
    ///
    /// # No modifiers, and a position that is never absent
    ///
    /// A finger carries no `modifiers`: no platform reports keyboard state on a
    /// touch event, and a field the backends would have to invent is a field a
    /// consumer would believe. And `position` is a [`PhysicalPoint`] rather than
    /// an `Option` — a contact is *defined* by where it touches, including on
    /// the release, and there is no [`PointerMode::Locked`](crate::PointerMode)
    /// equivalent that could take the position away.
    Touch {
        /// Which window.
        window: WindowId,
        /// Which touchscreen. Every contact on one screen shares this — see
        /// [`ContactId`], which is the one that tells fingers apart.
        device: DeviceId,
        /// When it happened.
        time: EventTime,
        /// Which finger.
        contact: ContactId,
        /// What it just did.
        phase: TouchPhase,
        /// Where it is, in device pixels. On
        /// [`Cancelled`](TouchPhase::Cancelled) this is the last place the
        /// platform knew about rather than a place the user chose.
        position: PhysicalPoint,
    },

    /// Text was committed — by typing, by an input method, or by a paste the
    /// window system routed as text input.
    ///
    /// Separate from [`Key`](ShellEvent::Key) because the relationship is not
    /// one-to-one: an IME commits several characters from one keystroke, or one
    /// character from five, and a dead key commits nothing at all. A text field
    /// that reconstructs its content from key events is a text field that
    /// cannot type in Japanese.
    TextCommit {
        /// Which window.
        window: WindowId,
        /// When it happened.
        time: EventTime,
        /// The committed text. Never empty.
        text: String,
    },

    /// The set of monitors changed — hotplug, resolution change, or a layout
    /// rearrangement.
    ///
    /// Carries no payload: [`Shell::monitors`](crate::Shell::monitors) is the
    /// authority, and duplicating the list into the event would let a consumer
    /// hold a stale copy that looks authoritative. A window's *own* scale
    /// change arrives separately as
    /// [`ScaleFactorChanged`](ShellEvent::ScaleFactorChanged).
    MonitorsChanged,

    /// A file was dropped on the window.
    ///
    /// One event per file: a multi-file drop produces several. Only delivered
    /// when the window was created with
    /// [`WindowDesc::accept_drops`](crate::WindowDesc::accept_drops) and
    /// [`ShellCaps::DRAG_DROP`](crate::ShellCaps::DRAG_DROP) is set.
    DroppedFile {
        /// Which window.
        window: WindowId,
        /// When it happened.
        time: EventTime,
        /// The dropped path. Not guaranteed to exist or to be readable — it
        /// comes from another process, and checking is the consumer's job.
        path: PathBuf,
        /// Where in the window it was dropped, if the backend said.
        position: Option<PhysicalPoint>,
    },

    /// The answer to a [`clipboard_request`](crate::Shell::clipboard_request).
    ///
    /// **Exactly one of these arrives for every accepted request**, and it is
    /// the only thing that ever answers one. It may be several frames late: a
    /// backend holds a read until it can answer it rather than reporting an
    /// empty clipboard it has not actually looked at. See
    /// [`clipboard`](crate::clipboard) for why reads are asynchronous and why
    /// there is no "ask again later" outcome.
    ClipboardData {
        /// Which window asked.
        window: WindowId,
        /// Which request this answers.
        request: ClipboardRequestId,
        /// The format that was delivered, spelled the way the *other*
        /// application spelled it.
        ///
        /// A [`ReceivedMime`] rather than a
        /// [`MimeType`](crate::MimeType) because the peer chose this string,
        /// not us: asking for `text/plain;charset=utf-8` and being answered
        /// with `text/plain` is normal and must round-trip verbatim. Compare
        /// with [`ReceivedMime::matches`], never with string equality.
        ///
        /// For [`Empty`](ClipboardContent::Empty) and
        /// [`Unavailable`](ClipboardContent::Unavailable) there is no peer
        /// spelling to report, so this is the format that was *asked* for.
        mime: ReceivedMime,
        /// What the read produced — bytes, an empty clipboard, or a failure.
        /// See [`ClipboardContent`], which exists because the three are not one
        /// case.
        content: ClipboardContent,
    },
}

impl ShellEvent {
    /// Which window this event concerns, if any.
    ///
    /// `None` only for [`MonitorsChanged`](ShellEvent::MonitorsChanged), which
    /// is a property of the session rather than of a window. A multi-window
    /// consumer routes on this.
    #[must_use]
    pub const fn window(&self) -> Option<WindowId> {
        match self {
            Self::Resized { window, .. }
            | Self::ScaleFactorChanged { window, .. }
            | Self::CloseRequested { window }
            | Self::WindowDestroyed { window }
            | Self::Focus { window, .. }
            | Self::Key { window, .. }
            | Self::PointerMotion { window, .. }
            | Self::PointerFocus { window, .. }
            | Self::Button { window, .. }
            | Self::Wheel { window, .. }
            | Self::Touch { window, .. }
            | Self::TextCommit { window, .. }
            | Self::DroppedFile { window, .. }
            | Self::ClipboardData { window, .. } => Some(*window),
            Self::MonitorsChanged => None,
        }
    }

    /// When this event happened, for the variants that are input samples.
    ///
    /// `None` for state transitions; see the [module docs](self) for why they
    /// deliberately have no timestamp.
    #[must_use]
    pub const fn time(&self) -> Option<EventTime> {
        match self {
            Self::Key { time, .. }
            | Self::PointerMotion { time, .. }
            | Self::PointerFocus { time, .. }
            | Self::Button { time, .. }
            | Self::Wheel { time, .. }
            | Self::Touch { time, .. }
            | Self::TextCommit { time, .. }
            | Self::DroppedFile { time, .. } => Some(*time),
            Self::Resized { .. }
            | Self::ScaleFactorChanged { .. }
            | Self::CloseRequested { .. }
            | Self::WindowDestroyed { .. }
            | Self::Focus { .. }
            | Self::MonitorsChanged
            | Self::ClipboardData { .. } => None,
        }
    }

    /// Whether this event feeds the input pipeline rather than the window
    /// lifecycle.
    ///
    /// The P2 action layer subscribes to exactly this subset.
    #[inline]
    #[must_use]
    pub const fn is_input(&self) -> bool {
        self.time().is_some()
    }

    /// A short, stable name for logs and test assertions.
    ///
    /// Stable because tests assert on sequences of these: a scripted
    /// `HeadlessShell` session compares `["Resized", "Focus", "Key", …]`, which
    /// stays readable in a failure message in a way that a `Debug` dump of the
    /// full payloads does not.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Resized { .. } => "Resized",
            Self::ScaleFactorChanged { .. } => "ScaleFactorChanged",
            Self::CloseRequested { .. } => "CloseRequested",
            Self::WindowDestroyed { .. } => "WindowDestroyed",
            Self::Focus { .. } => "Focus",
            Self::Key { .. } => "Key",
            Self::PointerMotion { .. } => "PointerMotion",
            Self::PointerFocus { .. } => "PointerFocus",
            Self::Button { .. } => "Button",
            Self::Wheel { .. } => "Wheel",
            Self::Touch { .. } => "Touch",
            Self::TextCommit { .. } => "TextCommit",
            Self::MonitorsChanged => "MonitorsChanged",
            Self::DroppedFile { .. } => "DroppedFile",
            Self::ClipboardData { .. } => "ClipboardData",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;
    use crcbl_core::Pool;

    fn window_id() -> WindowId {
        let mut pool: Pool<u8> = Pool::new();
        pool.insert(0).cast()
    }

    fn key_event(time: EventTime) -> ShellEvent {
        ShellEvent::Key {
            window: window_id(),
            device: DeviceId(1),
            time,
            scancode: Scancode(57),
            key_code: Some(KeyCode::Space),
            keysym: Keysym::from_char(' '),
            state: ButtonState::Pressed,
            repeat: false,
            modifiers: Modifiers::empty(),
        }
    }

    #[test]
    fn a_clipboard_answer_carries_the_peers_spelling() {
        let event = ShellEvent::ClipboardData {
            window: window_id(),
            request: ClipboardRequestId(3),
            mime: ReceivedMime::new("text/plain"),
            content: ClipboardContent::Bytes(b"pasted".to_vec()),
        };
        let ShellEvent::ClipboardData { mime, content, .. } = &event else {
            panic!("wrong variant");
        };
        assert_eq!(mime.as_str(), "text/plain");
        assert!(mime.matches(crate::MimeType::TextUtf8));
        assert_eq!(content.bytes(), Some(&b"pasted"[..]));
        assert_eq!(content.text(), Some("pasted"));

        // The three outcomes a caller must be able to tell apart. An empty
        // *payload* is a successful read; an empty *clipboard* is not the same
        // thing; and neither is a failure.
        assert_eq!(ClipboardContent::Bytes(Vec::new()).bytes(), Some(&[][..]));
        assert_ne!(ClipboardContent::Bytes(Vec::new()), ClipboardContent::Empty);
        assert_eq!(ClipboardContent::Empty.bytes(), None);
        assert_eq!(ClipboardContent::Unavailable.bytes(), None);
        assert_ne!(ClipboardContent::Empty, ClipboardContent::Unavailable);
        assert_eq!(ClipboardContent::Unavailable.name(), "Unavailable");
        // Invalid UTF-8 is refused rather than lossily replaced.
        assert_eq!(ClipboardContent::Bytes(vec![0xff]).text(), None);
        assert_eq!(
            event.time(),
            None,
            "a clipboard answer is not an input sample"
        );
    }

    #[test]
    fn input_events_carry_a_usable_timestamp_and_lifecycle_events_do_not() {
        let pressed = key_event(EventTime::from_millis(1_000));
        let released = key_event(EventTime::from_millis(1_180));

        // This subtraction is the whole reason timestamps exist: 180 ms is a
        // "tap", not a "hold", and no frame boundary got a vote.
        let held = released
            .time()
            .expect("input")
            .saturating_since(pressed.time().expect("input"));
        assert_eq!(held, Duration::from_millis(180));

        assert!(pressed.is_input());
        assert!(!ShellEvent::MonitorsChanged.is_input());
        assert_eq!(
            ShellEvent::Resized {
                window: window_id(),
                size: PhysicalSize::new(1, 1),
                scale_factor: 1.0,
            }
            .time(),
            None
        );
    }

    #[test]
    fn every_variant_reports_its_window_and_a_stable_name() {
        let window = window_id();
        let events = [
            ShellEvent::Resized {
                window,
                size: PhysicalSize::new(640, 360),
                scale_factor: 1.0,
            },
            ShellEvent::ScaleFactorChanged {
                window,
                scale_factor: 2.0,
                size: PhysicalSize::new(1280, 720),
            },
            ShellEvent::CloseRequested { window },
            ShellEvent::WindowDestroyed { window },
            ShellEvent::Focus {
                window,
                focused: true,
            },
            key_event(EventTime::ZERO),
            ShellEvent::PointerMotion {
                window,
                device: DeviceId::UNKNOWN,
                time: EventTime::ZERO,
                abs: Some(PhysicalPoint::new(1.0, 2.0)),
                raw_delta: Some((0.5, -0.5)),
            },
            ShellEvent::PointerFocus {
                window,
                device: DeviceId::UNKNOWN,
                time: EventTime::ZERO,
                entered: true,
                position: None,
            },
            ShellEvent::Button {
                window,
                device: DeviceId::UNKNOWN,
                time: EventTime::ZERO,
                button: PointerButton::Left,
                state: ButtonState::Pressed,
                position: None,
                modifiers: Modifiers::CTRL,
            },
            ShellEvent::Wheel {
                window,
                device: DeviceId::UNKNOWN,
                time: EventTime::ZERO,
                delta: ScrollDelta::Lines { x: 0.0, y: 1.0 },
                position: None,
                modifiers: Modifiers::empty(),
            },
            ShellEvent::Touch {
                window,
                device: DeviceId::UNKNOWN,
                time: EventTime::ZERO,
                contact: ContactId(1),
                phase: TouchPhase::Began,
                position: PhysicalPoint::new(4.0, 5.0),
            },
            ShellEvent::TextCommit {
                window,
                time: EventTime::ZERO,
                text: "は".to_string(),
            },
            ShellEvent::DroppedFile {
                window,
                time: EventTime::ZERO,
                path: PathBuf::from("/tmp/scene.ron"),
                position: None,
            },
            ShellEvent::ClipboardData {
                window,
                request: ClipboardRequestId(1),
                mime: ReceivedMime::from(crate::MimeType::TextUtf8),
                content: ClipboardContent::Empty,
            },
            ShellEvent::MonitorsChanged,
        ];

        let mut names = Vec::new();
        for event in &events {
            names.push(event.name());
            match event {
                ShellEvent::MonitorsChanged => assert_eq!(event.window(), None),
                _ => assert_eq!(event.window(), Some(window), "{}", event.name()),
            }
        }
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "event names must be unique");
    }

    /// Two fingers are two contacts, and the event says which is which.
    ///
    /// The shape claim in one test: a batch of touch events keyed by
    /// [`ContactId`] can be reduced to a per-finger position, the two fingers do
    /// not share an id, and moving one does not move the other. This is what the
    /// seam is *for* — before it, a second finger had nowhere to go and the web
    /// backend dropped it.
    #[test]
    fn a_batch_of_contacts_keeps_the_two_fingers_apart() {
        let window = window_id();
        let touch = |contact: u32, phase: TouchPhase, x: f64, y: f64| ShellEvent::Touch {
            window,
            device: DeviceId(3),
            time: EventTime::from_millis(1),
            contact: ContactId(contact),
            phase,
            position: PhysicalPoint::new(x, y),
        };
        let batch = [
            touch(7, TouchPhase::Began, 10.0, 20.0),
            touch(9, TouchPhase::Began, 300.0, 400.0),
            touch(9, TouchPhase::Moved, 305.0, 410.0),
            touch(7, TouchPhase::Ended, 10.0, 20.0),
        ];

        let mut live: Vec<(ContactId, TouchPhase, PhysicalPoint)> = Vec::new();
        for event in &batch {
            let ShellEvent::Touch {
                contact,
                phase,
                position,
                ..
            } = event
            else {
                panic!("wrong variant");
            };
            live.retain(|(id, ..)| id != contact);
            if !phase.ends_contact() {
                live.push((*contact, *phase, *position));
            }
        }

        // One finger left, at the place *its own* moves put it — not at the
        // other's, and not at the average of the two.
        assert_eq!(
            live,
            vec![(
                ContactId(9),
                TouchPhase::Moved,
                PhysicalPoint::new(305.0, 410.0)
            )]
        );
        assert!(TouchPhase::Ended.ends_contact());
        assert!(TouchPhase::Cancelled.ends_contact());
        assert!(!TouchPhase::Began.ends_contact());
        assert!(!TouchPhase::Moved.ends_contact());
        // A cancel is not an end: they agree that the contact is over and
        // disagree about everything a consumer does next.
        assert_ne!(TouchPhase::Ended, TouchPhase::Cancelled);
        assert!(batch[0].is_input(), "a contact is an input sample");
        assert_eq!(batch[0].time(), Some(EventTime::from_millis(1)));
    }

    #[test]
    fn locked_pointer_motion_has_no_absolute_position() {
        // The invariant `PointerMode::Locked` documents, expressed as a value.
        let event = ShellEvent::PointerMotion {
            window: window_id(),
            device: DeviceId(2),
            time: EventTime::from_micros(16_666),
            abs: None,
            raw_delta: Some((12.0, -3.0)),
        };
        let ShellEvent::PointerMotion { abs, raw_delta, .. } = event else {
            panic!("wrong variant");
        };
        assert_eq!(abs, None);
        assert_eq!(raw_delta, Some((12.0, -3.0)));
    }
}
