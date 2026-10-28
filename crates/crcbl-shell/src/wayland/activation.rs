//! Activation tokens through `xdg-activation-v1`.
//!
//! A token is how one client lets another raise a window: a notification
//! daemon asks for one with the serial of the click on its toast, hands it to
//! the program the toast came from (the notification spec's
//! `ActivationToken` signal, or `XDG_ACTIVATION_TOKEN` for a launch), and that
//! program passes it to `xdg_activation_v1.activate`. The compositor decides
//! whether to honour it, and the serial is what it decides by: a token made
//! without one, or from a stale one, may be refused focus.
//!
//! The token arrives in an event, `xdg_activation_token_v1.done`, which
//! sway 1.12 and Hyprland 0.56.2 send in answer to `commit` (the e2e suite's
//! `a_click_on_a_panel_gives_an_activation_token`). So
//! [`WaylandShell::activation_token`] waits for it with blocking round trips
//! rather than adding an event to the seam: each costs one exchange with the
//! compositor, and a caller asks for a token once per click.

use crate::{ShellError, WindowId};

use super::protocol::xdg_activation::{xdg_activation_token_v1, xdg_activation_v1};
use super::{ObjectKind, RawEvent, WaylandShell};

/// The most round trips [`WaylandShell::activation_token`] waits through for
/// `done`. The protocol does not say when it comes; a compositor that has not
/// answered after these has no token for us.
pub(super) const ACTIVATION_ROUNDTRIPS: usize = 4;

impl WaylandShell {
    /// A token another client can raise its window with, made from `window`'s
    /// surface and the latest input serial a seat delivered to this client.
    ///
    /// `None` when the compositor has no `xdg_activation_v1`.
    ///
    /// # Errors
    ///
    /// [`ShellError::InvalidWindow`] for a stale handle, a lost connection, or
    /// [`ShellError::Backend`] when the compositor sent no token.
    pub(super) fn activation_token(
        &mut self,
        window: WindowId,
    ) -> Result<Option<String>, ShellError> {
        let surface = self.window(window)?.surface;
        let manager = self.conn.activation;
        if manager.is_null() {
            return Ok(None);
        }
        // SAFETY: the manager is a live global; `get_activation_token` takes
        // no arguments beyond the object it creates.
        let token = unsafe { xdg_activation_v1::get_activation_token(manager) };
        if token.is_null() {
            return Err(ShellError::Backend(
                "xdg_activation_v1.get_activation_token failed".to_string(),
            ));
        }
        self.conn.watch(token, ObjectKind::ActivationToken);
        // The seat that saw this client's input last, as a popup grab picks
        // it: a click on `window` is what a caller asks after.
        let seat = self
            .seats
            .iter()
            .filter(|seat| seat.last_serial != 0)
            .max_by_key(|seat| seat.last_serial)
            .map(|seat| (seat.proxy, seat.last_serial));
        // SAFETY: the token, the seat and the surface are live on this
        // connection.
        unsafe {
            if let Some((seat, serial)) = seat {
                xdg_activation_token_v1::set_serial(token, serial, seat);
            }
            xdg_activation_token_v1::set_surface(token, surface);
            xdg_activation_token_v1::commit(token);
        }
        let mut answer = None;
        for _ in 0..ACTIVATION_ROUNDTRIPS {
            if let Err(error) = self.conn.roundtrip() {
                // SAFETY: the token is live and `destroy` is its destructor.
                unsafe { self.conn.release(token, xdg_activation_token_v1::destroy) };
                return Err(error);
            }
            answer = take_done(&mut self.conn.sink().events, token as usize);
            if answer.is_some() {
                break;
            }
        }
        // SAFETY: the token is live and `destroy` is its destructor; the
        // string it gave is ours to keep after it is gone.
        unsafe { self.conn.release(token, xdg_activation_token_v1::destroy) };
        answer.map(Some).ok_or_else(|| {
            ShellError::Backend("the compositor sent no xdg-activation token".to_string())
        })
    }
}

/// Removes `token`'s `done` from the queued events and returns its string,
/// leaving every other event for the pump.
fn take_done(events: &mut Vec<RawEvent>, token: usize) -> Option<String> {
    let index = events.iter().position(
        |event| matches!(event, RawEvent::ActivationDone { token: from, .. } if *from == token),
    )?;
    match events.remove(index) {
        RawEvent::ActivationDone { value, .. } => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(token: usize, value: &str) -> RawEvent {
        RawEvent::ActivationDone {
            token,
            value: value.to_string(),
        }
    }

    /// **Only the asked token's answer is taken**, and every other event stays
    /// queued for the pump in its order.
    #[test]
    fn take_done_takes_only_its_own_answer() {
        let mut events = vec![
            RawEvent::GlobalRemove { name: 7 },
            done(1, "other"),
            done(2, "mine"),
            RawEvent::GlobalRemove { name: 8 },
        ];
        assert_eq!(take_done(&mut events, 2).as_deref(), Some("mine"));
        assert_eq!(take_done(&mut events, 2), None);
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0], RawEvent::GlobalRemove { name: 7 }));
        assert!(matches!(
            events[1],
            RawEvent::ActivationDone { token: 1, .. }
        ));
        assert!(matches!(events[2], RawEvent::GlobalRemove { name: 8 }));
    }
}
