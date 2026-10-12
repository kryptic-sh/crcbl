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
//!
//! # Outside the loop
//!
//! A host with a frame loop of its own takes the same source from
//! [`for_run`] and polls it itself; on Steam,
//! `steam::steam_input` builds the source with Steam's virtual pads skipped.
//! [`PadSelection`] wraps any source to keep the list of connected pads and
//! pass on one chosen pad's input.

use std::collections::BTreeMap;

use crcbl_input::{GamepadEvent, GamepadId, PadKind};

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

/// The platform's pad source for a run, or `None` — see the module docs:
/// what [`Loop::new`](super::Loop::new) picks, for a host polling pads in a
/// loop of its own. A headless run (`windowed == false`) gets none.
#[must_use]
pub fn for_run(windowed: bool) -> Option<Box<dyn PadSource>> {
    if windowed { platform() } else { None }
}

/// A pad source that keeps the list of connected pads and passes on the
/// input of one chosen pad — a settings screen's "use this controller".
///
/// Connections and disconnections always pass through, so a game still hears
/// a pad arrive or leave; a pad's state passes through only while no pad is
/// chosen or it is the chosen one. A pad is listed from its connection or its
/// first state, whichever comes first, until it disconnects. The choice
/// survives the chosen pad disconnecting: a backend that reuses an id for the
/// same slot (XInput does) hands it straight back, and until then no other
/// pad steers the game.
#[derive(Debug)]
pub struct PadSelection<S> {
    source: S,
    devices: BTreeMap<GamepadId, PadKind>,
    selected: Option<GamepadId>,
}

impl<S: PadSource> PadSelection<S> {
    /// `source` with no pad chosen: every pad's input passes.
    pub const fn new(source: S) -> Self {
        Self {
            source,
            devices: BTreeMap::new(),
            selected: None,
        }
    }

    /// The pads connected now, by id, with their kinds.
    pub fn devices(&self) -> impl Iterator<Item = (GamepadId, PadKind)> + '_ {
        self.devices.iter().map(|(&id, &kind)| (id, kind))
    }

    /// The chosen pad, or `None` when every pad's input passes.
    #[must_use]
    pub const fn selected(&self) -> Option<GamepadId> {
        self.selected
    }

    /// Chooses the pad whose input passes, or every pad with `None`. Returns
    /// `false`, changing nothing, if that is already the choice or the pad is
    /// not connected.
    pub fn select(&mut self, selected: Option<GamepadId>) -> bool {
        if self.selected == selected || selected.is_some_and(|id| !self.devices.contains_key(&id)) {
            return false;
        }
        self.selected = selected;
        true
    }
}

impl<S: PadSource> PadSource for PadSelection<S> {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        let Self {
            source,
            devices,
            selected,
        } = self;
        source.poll(&mut |event| {
            match event {
                GamepadEvent::Connected { id, kind } => {
                    devices.insert(id, kind);
                }
                GamepadEvent::Disconnected { id } => {
                    devices.remove(&id);
                }
                GamepadEvent::State { id, snapshot } => {
                    devices.insert(id, snapshot.kind);
                    if selected.is_some_and(|chosen| chosen != id) {
                        return;
                    }
                }
            }
            emit(event);
        });
    }
}

impl<S: PadSource + ?Sized> PadSource for Box<S> {
    fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
        (**self).poll(emit);
    }
}

/// XInput, or `None` with the reason logged.
#[cfg(windows)]
fn platform() -> Option<Box<dyn PadSource>> {
    XInputPads::load().map(|pads| Box::new(pads) as Box<dyn PadSource>)
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
pub(super) struct XInputPads {
    xinput: crcbl_input::xinput::XInput,
    /// The error the last poll returned, so a slot that keeps failing is
    /// logged when it starts and when it stops rather than once a frame.
    failing: Option<crcbl_input::xinput::XInputError>,
}

#[cfg(windows)]
impl XInputPads {
    /// XInput, or `None` with the reason logged.
    pub(super) fn load() -> Option<Self> {
        match crcbl_input::xinput::XInput::load() {
            Ok(xinput) => {
                log::info!("gamepads: polling XInput through {}", xinput.library_name());
                Some(Self {
                    xinput,
                    failing: None,
                })
            }
            Err(error) => {
                log::warn!("gamepads: {error}; running with no pads");
                None
            }
        }
    }

    /// Skips Steam's virtual pads from now on — see
    /// `crcbl_input::xinput::XInput::skip_steam_virtual_pads`.
    #[cfg(all(feature = "steam", target_pointer_width = "64"))]
    pub(super) fn skip_steam_virtual_pads(
        &mut self,
    ) -> Result<(), crcbl_input::xinput::XInputError> {
        self.xinput.skip_steam_virtual_pads(true)
    }
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

    /// A source that reports the next batch of events at each poll.
    struct Script(Vec<Vec<GamepadEvent>>);

    impl PadSource for Script {
        fn poll(&mut self, emit: &mut dyn FnMut(GamepadEvent)) {
            if !self.0.is_empty() {
                for event in self.0.remove(0) {
                    emit(event);
                }
            }
        }
    }

    fn state(id: u32, kind: PadKind) -> GamepadEvent {
        GamepadEvent::State {
            id: GamepadId(id),
            snapshot: crcbl_input::GamepadSnapshot::neutral(kind),
        }
    }

    fn poll(pads: &mut impl PadSource) -> Vec<GamepadEvent> {
        let mut events = Vec::new();
        pads.poll(&mut |event| events.push(event));
        events
    }

    /// **Every pad is listed, and only the chosen one's input passes.**
    /// Connections and disconnections pass whatever is chosen; a pad first
    /// seen by its state is listed too; the choice outlives a disconnect.
    #[test]
    fn a_selection_lists_every_pad_and_passes_the_chosen_ones_input() {
        let connect = |id: u32| GamepadEvent::Connected {
            id: GamepadId(id),
            kind: PadKind::Xbox,
        };
        let source: Box<dyn PadSource> = Box::new(Script(vec![
            vec![connect(1), state(2, PadKind::PlayStation)],
            vec![state(1, PadKind::Xbox), state(2, PadKind::PlayStation)],
            vec![
                GamepadEvent::Disconnected { id: GamepadId(1) },
                state(2, PadKind::PlayStation),
                connect(3),
            ],
        ]));
        let mut pads = PadSelection::new(source);
        assert_eq!(
            poll(&mut pads),
            [connect(1), state(2, PadKind::PlayStation)],
            "with none chosen everything passes"
        );
        assert_eq!(
            pads.devices().collect::<Vec<_>>(),
            [
                (GamepadId(1), PadKind::Xbox),
                (GamepadId(2), PadKind::PlayStation)
            ]
        );
        assert!(!pads.select(Some(GamepadId(9))), "an absent pad is refused");
        assert!(pads.select(Some(GamepadId(1))));
        assert!(!pads.select(Some(GamepadId(1))), "already chosen");
        assert_eq!(pads.selected(), Some(GamepadId(1)));
        assert_eq!(poll(&mut pads), [state(1, PadKind::Xbox)]);
        assert_eq!(
            poll(&mut pads),
            [GamepadEvent::Disconnected { id: GamepadId(1) }, connect(3)],
            "pad 2 is still silenced after pad 1 leaves"
        );
        assert_eq!(pads.selected(), Some(GamepadId(1)));
        assert_eq!(
            pads.devices().map(|(id, _)| id).collect::<Vec<_>>(),
            [GamepadId(2), GamepadId(3)]
        );
        assert!(pads.select(None));
    }
}
