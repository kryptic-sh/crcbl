//! The loop's Steam limb (Steamworks slice 8): Steam pumped once
//! a frame, its overlay treated as a focus loss, its events handed to the
//! game, and Steam Input as the loop's pad source.
//!
//! **The game owns [`Steam`](crcbl_steam::Steam), and lends it** through
//! [`HostedGame::steam`] — as it lends its action map through
//! [`HostedGame::actions`] — because the game is what creates lobbies, opens
//! transports and unlocks achievements, from every hook it has. What the loop
//! takes off its hands is the per-frame part:
//!
//! - **the pump**, under [`STEAM_SPAN`](crate::perf::STEAM_SPAN) inside the
//!   frame's input span, after the shell's events and before the pads — so a
//!   `SteamPads` sees the device callbacks this pump drained;
//! - **the overlay**: `SteamEvent::OverlayActivated { active: true }` is a
//!   focus loss, taken the same frame through the path the window's own takes
//!   — held keys, buttons and contacts released through the game's paths, pads
//!   released, the game paused. Closing it resumes nothing: that stays the
//!   player's, as it is after alt-tab;
//! - **the events**, every one of them — the overlay's included — to
//!   [`HostedGame::steam_event`], in order.
//!
//! Steam Input reaches the loop as a pad source: [`steam_input`] turns a
//! `SteamPads` into one for [`Loop::set_pad_source`](super::Loop::set_pad_source),
//! with XInput beside it on Windows told to skip Steam's virtual pads, so
//! every press arrives once.
//!
//! # Where it exists
//!
//! With the umbrella's `steam` feature, on the targets `crcbl-steam` has items
//! for — 64-bit Linux, Windows and macOS. A game that overrides the two hooks
//! writes the same `cfg` on its overrides, since elsewhere the hooks, and the
//! types they name, do not exist.

use crcbl_input::GamepadEvent;
use crcbl_steam::{SteamEvent, SteamPads};

use super::{HostedGame, PadSource};

/// Steam as the loop pumps it: [`crcbl_steam::Steam`], or a test's script.
pub trait SteamSource {
    /// Drains Steam once — `Steam::pump` — and hands every event it decoded
    /// to `emit`, oldest first.
    fn pump(&mut self, emit: &mut dyn FnMut(SteamEvent));
}

impl SteamSource for crcbl_steam::Steam {
    fn pump(&mut self, emit: &mut dyn FnMut(SteamEvent)) {
        Self::pump(self);
        for event in self.events() {
            emit(event);
        }
    }
}

impl PadSource for SteamPads {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        Self::poll(self, emit);
    }
}

/// Pumps the game's Steam, if it lends one, hands it every event, and answers
/// whether the overlay opened — which the loop takes as a focus loss.
pub(super) fn pump<G: HostedGame>(game: &mut G) -> bool {
    let mut events = Vec::new();
    {
        let Some(steam) = game.steam() else {
            return false;
        };
        let _pumping = crcbl_core::trace::span(crate::perf::STEAM_SPAN);
        steam.pump(&mut |event| events.push(event));
    }
    let mut overlay_opened = false;
    for event in &events {
        overlay_opened |= matches!(event, SteamEvent::OverlayActivated { active: true });
        game.steam_event(event);
    }
    overlay_opened
}

/// Steam Input as the loop's pad source, for
/// [`Loop::set_pad_source`](super::Loop::set_pad_source): `pads` first, then —
/// on Windows, where the loop's own source is XInput — XInput, with Steam's
/// virtual pads skipped so a pad Steam reports is not reported twice.
///
/// If XInput cannot skip them (`xinput9_1_0.dll` cannot read a pad's vendor),
/// it is left out, with a warning: every press arriving twice is worse than a
/// pad Steam Input does not handle going unheard. Elsewhere `pads` is the
/// whole source, and it **replaces** the loop's native one: evdev (Linux) and
/// GameController (macOS) read Steam Input's virtual pad like any other and
/// have no way yet to skip it, so polling either beside Steam Input would
/// report every press twice.
#[must_use]
pub fn steam_input(pads: SteamPads) -> Box<dyn PadSource> {
    Box::new(Beside {
        first: pads,
        second: native_beside_steam(),
    })
}

/// XInput with Steam's virtual pads skipped, or `None` with the reason
/// logged.
#[cfg(windows)]
fn native_beside_steam() -> Option<Box<dyn PadSource>> {
    let mut xinput = super::pads::XInputPads::load()?;
    match xinput.skip_steam_virtual_pads() {
        Ok(()) => Some(Box::new(xinput)),
        Err(error) => {
            log::warn!("gamepads: {error}; polling Steam Input alone, without XInput");
            None
        }
    }
}

/// Nothing beside Steam Input on this target: its native backend, where it
/// has one, cannot skip Steam's virtual pads (see [`steam_input`]).
#[cfg(not(windows))]
fn native_beside_steam() -> Option<Box<dyn PadSource>> {
    None
}

/// Two pad sources polled as one: `first`, then `second`.
struct Beside<A> {
    first: A,
    second: Option<Box<dyn PadSource>>,
}

impl<A: PadSource> PadSource for Beside<A> {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        self.first.poll(emit);
        if let Some(second) = &mut self.second {
            second.poll(emit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_input::{GamepadId, PadKind};

    /// A pad source that reports one connection under its own id.
    struct One(u32);

    impl PadSource for One {
        fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
            emit(GamepadEvent::Connected {
                id: GamepadId(self.0),
                kind: PadKind::Generic,
            });
        }
    }

    fn polled(source: &mut dyn PadSource) -> Vec<GamepadEvent> {
        let mut events = Vec::new();
        source.poll(&mut |event| events.push(event));
        events
    }

    /// Steam Input's pads come first, then the native source's, each once.
    #[test]
    fn two_sources_beside_each_other_poll_in_order() {
        let mut both = Beside {
            first: One(1),
            second: Some(Box::new(One(2))),
        };
        let connected = |id| GamepadEvent::Connected {
            id: GamepadId(id),
            kind: PadKind::Generic,
        };
        assert_eq!(polled(&mut both), [connected(1), connected(2)]);
        let mut alone = Beside {
            first: One(1),
            second: None,
        };
        assert_eq!(polled(&mut alone), [connected(1)]);
    }
}
