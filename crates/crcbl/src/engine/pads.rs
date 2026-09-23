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

/// No backend on this target: said once, at start-up.
#[cfg(not(windows))]
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
        if failing != self.failing {
            match failing {
                Some(error) => log::warn!("gamepads: {error}"),
                None => log::info!("gamepads: XInput is answering every slot again"),
            }
            self.failing = failing;
        }
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
