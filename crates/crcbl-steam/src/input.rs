//! Steam Input onto the gamepad seam: controllers Steam maps, reported as the
//! same `crcbl_input::GamepadEvent`s every other pad backend reports.
//!
//! Steam Input is an action mapper, not a pad API. So the action manifest
//! this backend registers ([`PAD_MANIFEST`]) declares **one action set whose
//! actions are a neutral pad** — a digital action per positional
//! `PadButton`, a `joystick_move` action per stick and an analog trigger
//! action per trigger — and Steam's configurator maps whatever the player
//! holds (a Deck, a DualSense, a Switch Pro pad) onto it, remaps included.
//! Each [`SteamPads::poll`] reads those actions per controller into a
//! `GamepadSnapshot`, by the seam's conventions, so a game cannot tell these
//! events from XInput's (`docs/plan/42-steam.md`, "Input").
//!
//! ```text
//! SteamPads::open(&mut steam, manifest)  ── Init(true), the manifest, EnableDeviceCallbacks
//! each frame: steam.pump()               ── RunFrame(true), then device callbacks queued here
//!             pads.poll(|event| …)       ── Connected / State / Disconnected
//! drop(pads)                             ── Shutdown
//! ```
//!
//! # One owner per pad
//!
//! With Steam Input active, Steam can also present a virtual XInput pad for a
//! controller this backend reports. While a `SteamPads` is open, a game that
//! also polls `crcbl_input::xinput` turns its Steam-pad filter on
//! (`XInput::skip_steam_virtual_pads`), so every press arrives once.
//!
//! # Glyphs
//!
//! [`SteamPads::glyph`] answers the path of the PNG Steam draws for whatever
//! the player's configuration binds a pad control to — a Deck's `A`, a
//! DualSense's cross, or the key a remap moved it to — for a hint to show.
//!
//! # What is believed rather than checked
//!
//! - **The by-value returns.** `GetDigitalActionData` and
//!   `GetAnalogActionData` return `pack(1)` structs by value; that rustc
//!   returns them as each target's C compiler does is reasoned from the ABIs,
//!   and only a real controller on each target confirms it.
//! - **Stick Y.** Steam's `joystick_move` is taken to report +Y up, as XInput
//!   does, and is passed through unflipped; the Deck run pins it.
//! - **Action handles before the configuration loads.** The handles are
//!   looked up again on every poll while Steam answers `0` for them, rather
//!   than once at open, in case Steam answers `0` until a controller's
//!   configuration has loaded.

use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::{CStr, CString},
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use crcbl_input::{
    GamepadEvent, GamepadId, GamepadSnapshot, PadAxis, PadButton, PadKind, Stick, Trigger,
};

use crate::{
    Steam, SteamError,
    client::Client,
    ffi::{
        InputActionSetHandle, InputAnalogActionHandle, InputDigitalActionHandle, InputHandle,
        structs::{InputAnalogActionData, InputDigitalActionData},
    },
};

/// The action manifest [`SteamPads`] needs, as text: a game ships it as
/// [`PAD_MANIFEST_FILE`] and passes its absolute path to [`SteamPads::open`].
pub const PAD_MANIFEST: &str = include_str!("../assets/crcbl_pad.vdf");

/// The file name [`PAD_MANIFEST`] is shipped under.
pub const PAD_MANIFEST_FILE: &str = "crcbl_pad.vdf";

/// The manifest's one action set: the neutral pad.
const ACTION_SET: &CStr = c"pad";

/// Each digital action, and the button it is. The Guide button has none:
/// Steam keeps it for its own overlay.
const BUTTONS: [(&CStr, PadButton); 14] = [
    (c"south", PadButton::South),
    (c"east", PadButton::East),
    (c"west", PadButton::West),
    (c"north", PadButton::North),
    (c"left_shoulder", PadButton::LeftShoulder),
    (c"right_shoulder", PadButton::RightShoulder),
    (c"left_stick_click", PadButton::LeftStick),
    (c"right_stick_click", PadButton::RightStick),
    (c"start", PadButton::Start),
    (c"select", PadButton::Select),
    (c"dpad_up", PadButton::DpadUp),
    (c"dpad_down", PadButton::DpadDown),
    (c"dpad_left", PadButton::DpadLeft),
    (c"dpad_right", PadButton::DpadRight),
];

/// Each stick's `joystick_move` action.
const STICKS: [(&CStr, Stick); 2] = [(c"left_stick", Stick::Left), (c"right_stick", Stick::Right)];

/// Each trigger's analog trigger action.
const TRIGGERS: [(&CStr, Trigger); 2] = [
    (c"left_trigger", Trigger::Left),
    (c"right_trigger", Trigger::Right),
];

/// `STEAM_INPUT_MAX_ORIGINS` (`isteaminput.h`): the size of the buffer
/// `GetDigitalActionOrigins` and `GetAnalogActionOrigins` fill.
pub(crate) const MAX_ORIGINS: usize = 8;

/// A control on the neutral pad, for [`SteamPads::glyph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadControl {
    /// A button.
    Button(PadButton),
    /// A stick.
    Stick(Stick),
    /// A trigger.
    Trigger(Trigger),
}

/// How big a glyph is (`ESteamInputGlyphSize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphSize {
    /// 32 × 32 pixels (`k_ESteamInputGlyphSize_Small`).
    Small,
    /// 128 × 128 pixels (`k_ESteamInputGlyphSize_Medium`).
    Medium,
    /// 256 × 256 pixels (`k_ESteamInputGlyphSize_Large`).
    Large,
}

/// `ESteamInputType`'s values (`isteaminput.h`) that name a family.
mod input_type {
    pub(super) const XBOX_360: i32 = 2;
    pub(super) const XBOX_ONE: i32 = 3;
    pub(super) const PS4: i32 = 5;
    pub(super) const SWITCH_JOY_CON_PAIR: i32 = 8;
    pub(super) const SWITCH_JOY_CON_SINGLE: i32 = 9;
    pub(super) const SWITCH_PRO: i32 = 10;
    pub(super) const PS3: i32 = 12;
    pub(super) const PS5: i32 = 13;
    pub(super) const STEAM_DECK: i32 = 14;
    pub(super) const SWITCH_2_PRO: i32 = 16;
}

/// A controller's family, from `GetInputTypeForHandle`. Anything unnamed —
/// a Steam Controller, a generic DirectInput pad, a type a newer SDK adds —
/// is [`PadKind::Generic`].
const fn kind_of(input_type: i32) -> PadKind {
    match input_type {
        input_type::XBOX_360 | input_type::XBOX_ONE => PadKind::Xbox,
        input_type::PS3 | input_type::PS4 | input_type::PS5 => PadKind::PlayStation,
        input_type::SWITCH_JOY_CON_PAIR
        | input_type::SWITCH_JOY_CON_SINGLE
        | input_type::SWITCH_PRO
        | input_type::SWITCH_2_PRO => PadKind::Switch,
        input_type::STEAM_DECK => PadKind::SteamDeck,
        _ => PadKind::Generic,
    }
}

/// Why [`SteamPads::open`] failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InputError {
    /// A [`SteamPads`] is already open: `ISteamInput` has one `Init` and one
    /// `Shutdown`, and two owners would shut it down under each other.
    #[error("Steam Input is already open")]
    AlreadyOpen,
    /// The manifest path is not one Steam can be handed: it must be absolute
    /// (`SetInputActionManifestFilePath` takes an absolute path), name a
    /// file, and be UTF-8.
    #[error("{}: {reason}", path.display())]
    Manifest {
        /// The path given.
        path: PathBuf,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// Steam refused a call, or the path held a NUL.
    #[error(transparent)]
    Steam(#[from] SteamError),
}

/// The device changes the pump drains for the open [`SteamPads`]:
/// `(InputHandle_t, connected)`, oldest first.
#[derive(Debug, Default)]
pub(crate) struct PadQueue(RefCell<VecDeque<(InputHandle, bool)>>);

impl PadQueue {
    /// Queues one `SteamInputDeviceConnected_t` or `…Disconnected_t`.
    pub(crate) fn push(&self, handle: InputHandle, connected: bool) {
        self.0.borrow_mut().push_back((handle, connected));
    }
}

/// The manifest's handles, `0` until Steam answers one.
#[derive(Debug, Default)]
struct Handles {
    set: InputActionSetHandle,
    buttons: [InputDigitalActionHandle; BUTTONS.len()],
    sticks: [InputAnalogActionHandle; STICKS.len()],
    triggers: [InputAnalogActionHandle; TRIGGERS.len()],
}

/// One controller Steam has reported, and what was last reported for it.
#[derive(Debug, Clone, Copy)]
struct Pad {
    handle: InputHandle,
    /// Kept across a disconnect: Steam's handle names the controller, not a
    /// slot, so a controller that comes back is the same pad.
    id: GamepadId,
    kind: PadKind,
    connected: bool,
    last: GamepadSnapshot,
}

/// What one poll read for a controller: every action's data, an unbound or
/// unresolved one left inactive.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Reading {
    pub(crate) buttons: [InputDigitalActionData; BUTTONS.len()],
    pub(crate) sticks: [InputAnalogActionData; STICKS.len()],
    pub(crate) triggers: [InputAnalogActionData; TRIGGERS.len()],
}

/// A stick axis, clamped to the seam's −1…1; `None` if Steam's value is not
/// finite.
fn stick_axis(value: f32) -> Option<f32> {
    value.is_finite().then(|| value.clamp(-1.0, 1.0))
}

/// A trigger, clamped to the seam's 0…1; `None` if Steam's value is not
/// finite.
fn trigger_axis(value: f32) -> Option<f32> {
    value.is_finite().then(|| value.clamp(0.0, 1.0))
}

/// One controller's actions as the seam's snapshot, and how many axis values
/// were not finite (each read as `0.0`). An inactive action — unbound in the
/// active set — is released or centred.
pub(crate) fn snapshot_of(kind: PadKind, reading: &Reading) -> (GamepadSnapshot, u32) {
    let mut snapshot = GamepadSnapshot::neutral(kind);
    let mut rejected = 0;
    let mut set = |axis: PadAxis, value: Option<f32>| match value {
        Some(value) => snapshot.axes[axis as usize] = value,
        None => rejected += 1,
    };
    for (data, &(_, stick)) in reading.sticks.iter().zip(&STICKS) {
        let data = *data;
        if data.active == 0 {
            continue;
        }
        let (x, y) = match stick {
            Stick::Left => (PadAxis::LeftX, PadAxis::LeftY),
            Stick::Right => (PadAxis::RightX, PadAxis::RightY),
        };
        set(x, stick_axis(data.x));
        set(y, stick_axis(data.y));
    }
    for (data, &(_, trigger)) in reading.triggers.iter().zip(&TRIGGERS) {
        let data = *data;
        if data.active == 0 {
            continue;
        }
        let axis = match trigger {
            Trigger::Left => PadAxis::LeftTrigger,
            Trigger::Right => PadAxis::RightTrigger,
        };
        set(axis, trigger_axis(data.x));
    }
    snapshot.buttons = reading
        .buttons
        .iter()
        .zip(&BUTTONS)
        .filter(|(data, _)| data.active != 0 && data.state != 0)
        .map(|(_, &(_, button))| button)
        .collect();
    (snapshot, rejected)
}

/// Steam Input, open: the controllers Steam maps, polled onto the gamepad
/// seam. See the module docs.
///
/// One at a time per [`Steam`] ([`InputError::AlreadyOpen`]). `!Send`, like
/// `Steam`: poll it on the pump thread, after [`Steam::pump`] — the pump
/// runs `ISteamInput::RunFrame` and hands over the device callbacks. Dropping
/// it shuts Steam Input down; the pads it reported get no disconnection, so a
/// map fed from it holds their last state until it is released.
pub struct SteamPads {
    client: Arc<Client>,
    queue: Rc<PadQueue>,
    handles: Handles,
    pads: Vec<Pad>,
    rejected_axes: u64,
    _not_send: PhantomData<*const ()>,
}

impl std::fmt::Debug for SteamPads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SteamPads")
            .field("pads", &self.pads)
            .field("rejected_axes", &self.rejected_axes)
            .finish_non_exhaustive()
    }
}

impl SteamPads {
    /// Opens Steam Input: `Init` with the pump owning `RunFrame`, the action
    /// manifest at `manifest` (`SetInputActionManifestFilePath`), and device
    /// callbacks on — so every controller already connected is reported by
    /// the first poll after the next pump.
    ///
    /// `manifest` is the absolute path of a file holding [`PAD_MANIFEST`];
    /// Steam reads it, and remembers it for the rest of its session.
    ///
    /// # Errors
    ///
    /// [`InputError::AlreadyOpen`] while another is open;
    /// [`InputError::Manifest`] for a path Steam cannot be given;
    /// [`SteamError::Refused`] when Steam refuses `Init` or the manifest —
    /// after a refused manifest, Steam Input is shut down again.
    pub fn open(steam: &mut Steam, manifest: &Path) -> Result<Self, InputError> {
        Self::open_at(steam, &manifest_path(manifest)?)
    }

    /// [`open`](Self::open) with the path already checked — the part the
    /// tests run, since a filesystem check cannot run under Miri.
    fn open_at(steam: &mut Steam, path: &CStr) -> Result<Self, InputError> {
        if steam.pads.strong_count() > 0 {
            return Err(InputError::AlreadyOpen);
        }
        let client = Arc::clone(&steam.client);
        let input = &client.lib.fns.input;
        // SAFETY: `client.input` is the non-null interface init resolved, and
        // `Steam` is `!Send`, so this is the pump thread.
        if !unsafe { (input.init)(client.input, true) } {
            return Err(SteamError::Refused("ISteamInput::Init").into());
        }
        // SAFETY: as above; `path` is NUL-terminated and outlives the call.
        if !unsafe { (input.set_input_action_manifest_file_path)(client.input, path.as_ptr()) } {
            // SAFETY: as above; balances the `Init` that succeeded.
            unsafe { (input.shutdown)(client.input) };
            return Err(SteamError::Refused("SetInputActionManifestFilePath").into());
        }
        // SAFETY: as above.
        unsafe { (input.enable_device_callbacks)(client.input) };
        let queue = Rc::new(PadQueue::default());
        steam.pads = Rc::downgrade(&queue);
        Ok(Self {
            client,
            queue,
            handles: Handles::default(),
            pads: Vec::new(),
            rejected_axes: 0,
            _not_send: PhantomData,
        })
    }

    /// Reads every controller and calls `emit` with what changed since the
    /// last poll: connections and disconnections as Steam reported them, and
    /// each connected controller's snapshot when it differs from the last one
    /// reported — the same stream `crcbl_input::xinput` produces.
    ///
    /// Call once a frame, after [`Steam::pump`].
    pub fn poll(&mut self, mut emit: impl FnMut(GamepadEvent)) {
        self.resolve();
        let changes: Vec<_> = self.queue.0.borrow_mut().drain(..).collect();
        for (handle, connected) in changes {
            if connected {
                self.connect(handle, &mut emit);
            } else {
                self.disconnect(handle, &mut emit);
            }
        }
        for index in 0..self.pads.len() {
            let pad = self.pads[index];
            if !pad.connected {
                continue;
            }
            let reading = self.read(pad.handle);
            let (snapshot, rejected) = snapshot_of(pad.kind, &reading);
            self.rejected_axes += u64::from(rejected);
            if snapshot != pad.last {
                self.pads[index].last = snapshot;
                emit(GamepadEvent::State {
                    id: pad.id,
                    snapshot,
                });
            }
        }
    }

    /// How many stick or trigger values Steam has answered that were not
    /// finite, each read as centred. Must stay zero: a non-zero count is a
    /// broken library, or a by-value return this target gets wrong.
    #[must_use]
    pub fn rejected_axes(&self) -> u64 {
        self.rejected_axes
    }

    /// The path of the PNG Steam draws for what pad `id`'s configuration binds
    /// `control` to (`GetDigitalActionOrigins` or `GetAnalogActionOrigins`,
    /// then `GetGlyphPNGForActionOrigin` for the first origin), in Steam's
    /// default knockout style. `None` when `id` is not a connected pad here,
    /// the control is bound to nothing, or Steam has no image for it.
    ///
    /// The path is copied before this returns. `steam` is where a path that
    /// was not UTF-8 is counted, in
    /// [`PumpDiagnostics::lossy_strings`](crate::PumpDiagnostics::lossy_strings).
    #[must_use]
    pub fn glyph(
        &self,
        steam: &Steam,
        id: GamepadId,
        control: PadControl,
        size: GlyphSize,
    ) -> Option<PathBuf> {
        let pad = self.pads.iter().find(|pad| pad.id == id && pad.connected)?;
        let handles = &self.handles;
        if handles.set == 0 {
            return None;
        }
        let client = &self.client;
        let input = &client.lib.fns.input;
        let mut origins = [0_i32; MAX_ORIGINS];
        let out = origins.as_mut_ptr();
        let count = match control {
            PadControl::Button(button) => {
                let action = handle_for(&BUTTONS, &handles.buttons, button)?;
                // SAFETY: `client.input` is live, this is the pump thread, and
                // `out` has room for `STEAM_INPUT_MAX_ORIGINS` origins.
                unsafe {
                    (input.get_digital_action_origins)(
                        client.input,
                        pad.handle,
                        handles.set,
                        action,
                        out,
                    )
                }
            }
            PadControl::Stick(stick) => {
                let action = handle_for(&STICKS, &handles.sticks, stick)?;
                // SAFETY: as above.
                unsafe {
                    (input.get_analog_action_origins)(
                        client.input,
                        pad.handle,
                        handles.set,
                        action,
                        out,
                    )
                }
            }
            PadControl::Trigger(trigger) => {
                let action = handle_for(&TRIGGERS, &handles.triggers, trigger)?;
                // SAFETY: as above.
                unsafe {
                    (input.get_analog_action_origins)(
                        client.input,
                        pad.handle,
                        handles.set,
                        action,
                        out,
                    )
                }
            }
        };
        // `k_EInputActionOrigin_None` is zero: bound to nothing.
        let origin = origins[0];
        if count < 1 || origin == 0 {
            return None;
        }
        let size = match size {
            GlyphSize::Small => 0,
            GlyphSize::Medium => 1,
            GlyphSize::Large => 2,
        };
        // SAFETY: as above; `0` is `ESteamInputGlyphStyle_Knockout`.
        let path =
            unsafe { (input.get_glyph_png_for_action_origin)(client.input, origin, size, 0) };
        if path.is_null() {
            return None;
        }
        // SAFETY: straight out of the call, before any other Steam call.
        let path = unsafe { steam.copy_string(path) };
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    /// Looks up every handle Steam has not yet answered: the action set
    /// first, and the actions once it has one.
    fn resolve(&mut self) {
        let client = &self.client;
        let input = &client.lib.fns.input;
        let handles = &mut self.handles;
        if handles.set == 0 {
            // SAFETY: `client.input` is live, `SteamPads` is `!Send` so this is
            // the pump thread, and the name is NUL-terminated.
            handles.set =
                unsafe { (input.get_action_set_handle)(client.input, ACTION_SET.as_ptr()) };
            if handles.set == 0 {
                return;
            }
        }
        for (handle, (name, _)) in handles.buttons.iter_mut().zip(&BUTTONS) {
            if *handle == 0 {
                // SAFETY: as above.
                *handle = unsafe { (input.get_digital_action_handle)(client.input, name.as_ptr()) };
            }
        }
        let analog = handles
            .sticks
            .iter_mut()
            .zip(STICKS.iter().map(|(name, _)| name))
            .chain(
                handles
                    .triggers
                    .iter_mut()
                    .zip(TRIGGERS.iter().map(|(name, _)| name)),
            );
        for (handle, name) in analog {
            if *handle == 0 {
                // SAFETY: as above.
                *handle = unsafe { (input.get_analog_action_handle)(client.input, name.as_ptr()) };
            }
        }
    }

    /// Reports a controller Steam says connected: a known one keeps its id.
    /// A connection for one already connected changes nothing.
    fn connect(&mut self, handle: InputHandle, emit: &mut impl FnMut(GamepadEvent)) {
        let client = &self.client;
        // SAFETY: `client.input` is live, and this is the pump thread.
        let kind = kind_of(unsafe {
            (client.lib.fns.input.get_input_type_for_handle)(client.input, handle)
        });
        let id = match self.pads.iter_mut().find(|pad| pad.handle == handle) {
            Some(pad) if pad.connected => return,
            Some(pad) => {
                pad.connected = true;
                pad.kind = kind;
                pad.last = GamepadSnapshot::neutral(kind);
                pad.id
            }
            None => {
                let id = GamepadId::allocate();
                self.pads.push(Pad {
                    handle,
                    id,
                    kind,
                    connected: true,
                    last: GamepadSnapshot::neutral(kind),
                });
                id
            }
        };
        emit(GamepadEvent::Connected { id, kind });
    }

    /// Reports a controller Steam says disconnected, if it was connected.
    fn disconnect(&mut self, handle: InputHandle, emit: &mut impl FnMut(GamepadEvent)) {
        if let Some(pad) = self
            .pads
            .iter_mut()
            .find(|pad| pad.handle == handle && pad.connected)
        {
            pad.connected = false;
            emit(GamepadEvent::Disconnected { id: pad.id });
        }
    }

    /// Activates the pad action set on `handle` — cheap, and safe to repeat,
    /// per `isteaminput.h` — and reads every resolved action.
    fn read(&self, handle: InputHandle) -> Reading {
        let client = &self.client;
        let input = &client.lib.fns.input;
        let handles = &self.handles;
        let mut reading = Reading::default();
        if handles.set == 0 {
            return reading;
        }
        // SAFETY: `client.input` is live, this is the pump thread, and every
        // handle passed is one Steam answered.
        unsafe { (input.activate_action_set)(client.input, handle, handles.set) };
        for (data, &action) in reading.buttons.iter_mut().zip(&handles.buttons) {
            if action != 0 {
                // SAFETY: as above.
                *data = unsafe { (input.get_digital_action_data)(client.input, handle, action) };
            }
        }
        let analog = reading
            .sticks
            .iter_mut()
            .zip(&handles.sticks)
            .chain(reading.triggers.iter_mut().zip(&handles.triggers));
        for (data, &action) in analog {
            if action != 0 {
                // SAFETY: as above.
                *data = unsafe { (input.get_analog_action_data)(client.input, handle, action) };
            }
        }
        reading
    }
}

impl Drop for SteamPads {
    fn drop(&mut self) {
        let client = &self.client;
        // SAFETY: `client.input` is live and was `Init`ed by `open`; `SteamPads`
        // is `!Send`, so this is the pump thread.
        if !unsafe { (client.lib.fns.input.shutdown)(client.input) } {
            log::warn!("steam: ISteamInput::Shutdown answered false");
        }
    }
}

/// The resolved handle of `control` in a table of actions, if Steam has
/// answered one.
fn handle_for<T: PartialEq>(table: &[(&CStr, T)], handles: &[u64], control: T) -> Option<u64> {
    table
        .iter()
        .zip(handles)
        .find(|((_, known), _)| *known == control)
        .map(|(_, &handle)| handle)
        .filter(|&handle| handle != 0)
}

/// `manifest` as the C string `SetInputActionManifestFilePath` takes.
fn manifest_path(manifest: &Path) -> Result<CString, InputError> {
    let refuse = |reason| InputError::Manifest {
        path: manifest.to_path_buf(),
        reason,
    };
    if !manifest.is_absolute() {
        return Err(refuse("not an absolute path"));
    }
    if !manifest.is_file() {
        return Err(refuse("not a file"));
    }
    let text = manifest.to_str().ok_or_else(|| refuse("not UTF-8"))?;
    CString::new(text).map_err(|_| SteamError::InteriorNul("manifest").into())
}

#[cfg(test)]
mod tests;
