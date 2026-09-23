//! Where a [`Loop`](super::Loop)'s gamepad events come from.
//!
//! The loop polls one [`PadSource`] once a frame, beside the shell's pump, and
//! hands every event to its own menu map and to the game — see
//! [`HostedGame::gamepad_event`](super::HostedGame::gamepad_event). Which source
//! that is gets decided once, by [`Loop::new`](super::Loop::new), and a caller
//! with a reason of its own replaces it through
//! [`Loop::set_pad_source`](super::Loop::set_pad_source).
//!
//! # Which platforms have one
//!
//! - **Windows**: XInput, through `crcbl_input::xinput` (Windows-only, so it is
//!   not a doc link here: rustdoc on other targets cannot resolve it). A windowed
//!   run that cannot load it logs why once and runs with no pads.
//! - **Linux**: evdev, through `crcbl_input::evdev` (Linux-only, likewise). It
//!   cannot fail to start: `/dev/input` is listed at the first poll, and a
//!   failure there or on a pad is logged when it starts and when it stops.
//! - **macOS**: GameController.framework, through
//!   `crcbl_input::game_controller` (macOS-only, likewise). It discovers pads
//!   from the main run loop, which the AppKit shell's pump turns every frame; a
//!   windowed run that cannot find `GCController` logs why once and runs with no
//!   pads.
//! - **A browser** (`wasm32`): the Web Gamepad API, through
//!   `crcbl_input::web_gamepad` (`wasm32`-only, likewise), which reads what
//!   `web/engine/gamepad.js` reported from `navigator.getGamepads()` this
//!   frame. It cannot fail to start either. A pad connects on its first button
//!   press on the page, not when the page opens — the browser hides it until
//!   then — so each connection and disconnection is logged, and a pad the
//!   browser cannot standard-map is logged once and ignored.
//! - **Everywhere else**: no backend exists yet, and a windowed run logs that
//!   once and runs with no pads.
//! - **A headless run, on every target**, gets no source at all: a scripted or
//!   golden run must not be steered by whatever pad happens to be plugged into
//!   the machine running it.

use crcbl_input::GamepadEvent;

/// A source of [`GamepadEvent`]s, polled once a frame.
///
/// Every backend speaks the seam's conventions (see [`crcbl_input`]'s gamepad
/// docs), so the loop cannot tell which one spoke — and neither can a test,
/// which is the point: a test implements this over a script.
pub trait PadSource {
    /// Reads the pads once and calls `emit` with what changed since the last
    /// poll, in order.
    ///
    /// Infallible on purpose: a pad that stops answering is a
    /// [`GamepadEvent::Disconnected`], not a failed frame, and a source that
    /// hits an error it cannot express as one says so in the log itself.
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent));
}

/// The platform's pad source for a run, or `None` — see the module docs.
pub(super) fn for_run(windowed: bool) -> Option<Box<dyn PadSource>> {
    if windowed { platform() } else { None }
}

/// XInput, or `None` with the reason logged.
#[cfg(windows)]
fn platform() -> Option<Box<dyn PadSource>> {
    match crcbl_input::xinput::XInput::load() {
        Ok(xinput) => {
            log::info!("gamepads: polling XInput through {}", xinput.library_name());
            Some(Box::new(XInputPads {
                xinput,
                failing: None,
            }))
        }
        Err(error) => {
            log::warn!("gamepads: {error}; running with no pads");
            None
        }
    }
}

/// evdev, which has nothing to load: the first poll scans `/dev/input`.
#[cfg(target_os = "linux")]
fn platform() -> Option<Box<dyn PadSource>> {
    log::info!("gamepads: polling evdev pads under /dev/input");
    Some(Box::new(EvdevPads {
        evdev: crcbl_input::evdev::Evdev::new(),
        failing: None,
    }))
}

/// GameController.framework, or `None` with the reason logged.
#[cfg(target_os = "macos")]
fn platform() -> Option<Box<dyn PadSource>> {
    match crcbl_input::game_controller::GameController::new() {
        Ok(pads) => {
            log::info!("gamepads: polling GameController.framework");
            Some(Box::new(GameControllerPads(pads)))
        }
        Err(error) => {
            log::warn!("gamepads: {error}; running with no pads");
            None
        }
    }
}

/// The Web Gamepad API, which has nothing to load: the shim reports the pads
/// every frame.
#[cfg(target_arch = "wasm32")]
fn platform() -> Option<Box<dyn PadSource>> {
    log::info!(
        "gamepads: polling navigator.getGamepads(); a pad appears once one of its buttons is pressed on this page"
    );
    Some(Box::new(WebPads(
        crcbl_input::web_gamepad::WebGamepads::new(),
    )))
}

/// No backend on this target: said once, at start-up.
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "macos",
    target_arch = "wasm32"
)))]
fn platform() -> Option<Box<dyn PadSource>> {
    log::info!("gamepads: no pad backend exists for this target yet; running with no pads");
    None
}

/// [`crcbl_input::xinput::XInput`] as a [`PadSource`].
#[cfg(windows)]
struct XInputPads {
    xinput: crcbl_input::xinput::XInput,
    /// The error the last poll returned, so a slot that keeps failing is
    /// logged when it starts and when it stops rather than once a frame.
    failing: Option<crcbl_input::xinput::XInputError>,
}

#[cfg(windows)]
impl PadSource for XInputPads {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        // A failing slot has already been reported as disconnected and every
        // other slot was still polled (see `XInput::poll`), so what is left of
        // the error is the log line.
        let failing = self.xinput.poll(&mut *emit).err();
        note_failure(
            &mut self.failing,
            failing,
            "XInput is answering every slot again",
        );
    }
}

/// [`crcbl_input::evdev::Evdev`] as a [`PadSource`].
#[cfg(target_os = "linux")]
struct EvdevPads {
    evdev: crcbl_input::evdev::Evdev,
    /// The error the last poll returned, as `XInputPads` keeps its own.
    failing: Option<crcbl_input::evdev::EvdevError>,
}

#[cfg(target_os = "linux")]
impl PadSource for EvdevPads {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        // A pad that failed a read has already been reported as disconnected,
        // and everything else was still polled (see `Evdev::poll`), so what is
        // left of the error is the log line.
        let failing = self.evdev.poll(&mut *emit).err();
        note_failure(
            &mut self.failing,
            failing,
            "evdev is reading every pad again",
        );
    }
}

/// [`crcbl_input::game_controller::GameController`] as a [`PadSource`]. Its
/// poll cannot fail, so there is no error to keep.
#[cfg(target_os = "macos")]
struct GameControllerPads(crcbl_input::game_controller::GameController);

#[cfg(target_os = "macos")]
impl PadSource for GameControllerPads {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        self.0.poll(&mut *emit);
    }
}

/// [`crcbl_input::web_gamepad::WebGamepads`] as a [`PadSource`].
#[cfg(target_arch = "wasm32")]
struct WebPads(crcbl_input::web_gamepad::WebGamepads);

#[cfg(target_arch = "wasm32")]
impl PadSource for WebPads {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        // Connections are logged here and not on the desktop sources: a
        // browser pad appears on its first press rather than when the page
        // opens, and "is the page seeing my pad" is the first question a
        // player with one asks.
        let unmapped = self.0.poll(|event| {
            match event {
                GamepadEvent::Connected { id, kind } => {
                    log::info!("gamepads: pad {} connected ({kind:?})", id.0);
                }
                GamepadEvent::Disconnected { id } => {
                    log::info!("gamepads: pad {} disconnected", id.0);
                }
                GamepadEvent::State { .. } => {}
            }
            emit(event);
        });
        for pad in unmapped {
            log::warn!("gamepads: {pad}");
        }
    }
}

/// Logs a backend's poll error when it starts, changes or clears, rather than
/// once a frame, and keeps it in `last` to compare the next poll's against.
#[cfg(any(windows, target_os = "linux"))]
fn note_failure<E: std::fmt::Display + PartialEq>(
    last: &mut Option<E>,
    now: Option<E>,
    recovered: &str,
) {
    if now != *last {
        match &now {
            Some(error) => log::warn!("gamepads: {error}"),
            None => log::info!("gamepads: {recovered}"),
        }
        *last = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A headless run gets no pads on any target**, whatever is plugged in:
    /// the check is on the run, not on what the platform could load.
    #[test]
    fn a_headless_run_has_no_pad_source() {
        assert!(for_run(false).is_none());
    }
}
