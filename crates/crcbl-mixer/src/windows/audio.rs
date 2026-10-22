//! Everything the mixer follows on Windows, on its thread: the enumerator,
//! the active endpoints and their sessions, and the defaults.
//!
//! **A read patches the tables, then reads every value.** [`Audio::refresh`]
//! lists the active endpoints and each one's live sessions, opens and
//! registers for what is new, drops (and so unregisters) what has gone, and
//! keeps what stayed, with its number and its registrations; then it reads
//! each volume, mute and name and builds the [`MixerState`].
//!
//! **The default is the console role's**, `GetDefaultAudioEndpoint(…,
//! eConsole)`: what Windows calls the default device, which system sounds and
//! most applications play to, and the role lyr's bar follows.
//!
//! **One endpoint or session failing to answer is left out, not fatal.** A
//! device unplugged mid-read answers `AUDCLNT_E_DEVICE_INVALIDATED`, and the
//! notification of its removal follows; a driver that refuses one call should
//! not empty the whole mixer. Two failures end the mixer instead: the audio
//! service not running (`AUDCLNT_E_SERVICE_NOT_RUNNING`, from any call), and
//! the enumerator itself failing to list endpoints, after which nothing more
//! can be read. Both answer [`MixerError::Disconnected`].

use std::collections::HashSet;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume,
    MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::HRESULT;

use super::endpoint::{self, Endpoint};
use super::ids::Table;
use super::notify::DeviceWatch;
use super::session::{self, Session};
use super::{Message, com, policy_config};
use crate::model::{DeviceId, Direction, MixerState, StreamId};
use crate::{Action, MixerError, Target, level};

const DIRECTIONS: [Direction; 2] = [Direction::Output, Direction::Input];

const fn flow(direction: Direction) -> EDataFlow {
    match direction {
        Direction::Output => eRender,
        Direction::Input => eCapture,
    }
}

/// What a failed call means while reading: [`MixerError::Disconnected`]
/// ends the mixer; anything else leaves the one object out (`Ok(None)`).
fn or_skip<T>(result: windows::core::Result<T>) -> Result<Option<T>, MixerError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if com::is_gone(&error) => Err(MixerError::Disconnected),
        Err(_) => Ok(None),
    }
}

/// A volume control an action is applied to.
enum Control<'a> {
    Endpoint {
        key: &'a str,
        volume: &'a IAudioEndpointVolume,
    },
    Session(&'a ISimpleAudioVolume),
}

impl Control<'_> {
    fn scalar(&self) -> Result<f32, MixerError> {
        // SAFETY: plain reads.
        unsafe {
            match self {
                Self::Endpoint { volume, .. } => volume.GetMasterVolumeLevelScalar(),
                Self::Session(volume) => volume.GetMasterVolume(),
            }
        }
        .map_err(|error| com::failed("reading a volume", &error))
    }

    fn muted(&self) -> Result<bool, MixerError> {
        // SAFETY: plain reads.
        unsafe {
            match self {
                Self::Endpoint { volume, .. } => volume.GetMute(),
                Self::Session(volume) => volume.GetMute(),
            }
        }
        .map(|muted| muted.as_bool())
        .map_err(|error| com::failed("reading a mute", &error))
    }

    fn set_scalar(&self, scalar: f32) -> Result<(), MixerError> {
        // SAFETY: a scalar in 0..=1, which `level` holds it to, and no event
        // context.
        unsafe {
            match self {
                Self::Endpoint { volume, .. } => {
                    volume.SetMasterVolumeLevelScalar(scalar, std::ptr::null())
                }
                Self::Session(volume) => volume.SetMasterVolume(scalar, std::ptr::null()),
            }
        }
        .map_err(|error| com::failed("setting a volume", &error))
    }

    fn set_mute(&self, muted: bool) -> Result<(), MixerError> {
        // SAFETY: a plain value and no event context.
        unsafe {
            match self {
                Self::Endpoint { volume, .. } => volume.SetMute(muted, std::ptr::null()),
                Self::Session(volume) => volume.SetMute(muted, std::ptr::null()),
            }
        }
        .map_err(|error| com::failed("setting a mute", &error))
    }
}

/// The enumerator and every endpoint and session the mixer follows. Lives on
/// the mixer's thread, in the multithreaded apartment, and never leaves it.
pub(super) struct Audio {
    enumerator: IMMDeviceEnumerator,
    endpoints: Table<Endpoint>,
    sessions: Table<Session>,
    /// The default endpoint's id string for output, then input, as of the
    /// last read.
    defaults: [Option<String>; 2],
    changes: Sender<Message>,
    /// Declared last so it unregisters after the endpoints' and sessions'
    /// registrations, the order they were made in reversed.
    _devices: DeviceWatch,
}

impl Audio {
    /// The enumerator, registered for endpoint changes, with nothing read
    /// yet: [`refresh`](Self::refresh) reads.
    pub(super) fn open(changes: Sender<Message>) -> Result<Self, MixerError> {
        // SAFETY: a CLSID constant and no outer object; the result is checked.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|error| com::failed("making the device enumerator", &error))?;
        let devices = DeviceWatch::register(&enumerator, &changes)
            .map_err(|error| com::failed("registering for endpoint changes", &error))?;
        Ok(Self {
            enumerator,
            endpoints: Table::default(),
            sessions: Table::default(),
            defaults: [None, None],
            changes,
            _devices: devices,
        })
    }

    /// `direction`'s active endpoints.
    fn active(&self, direction: Direction) -> windows::core::Result<Vec<IMMDevice>> {
        // SAFETY: plain calls; the index stays below the count reported.
        unsafe {
            let listed = self
                .enumerator
                .EnumAudioEndpoints(flow(direction), DEVICE_STATE_ACTIVE)?;
            (0..listed.GetCount()?)
                .map(|index| listed.Item(index))
                .collect()
        }
    }

    /// The default endpoint's id string for `direction`, `None` when there
    /// is none.
    fn default_key(&self, direction: Direction) -> Result<Option<String>, MixerError> {
        // SAFETY: plain values.
        match unsafe {
            self.enumerator
                .GetDefaultAudioEndpoint(flow(direction), eConsole)
        } {
            Ok(device) => Ok(endpoint::key(&device)),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(None),
            Err(error) if com::is_gone(&error) => Err(MixerError::Disconnected),
            // An enumerator that cannot say which is the default shows none.
            Err(_) => Ok(None),
        }
    }

    /// Patches the tables to what Windows has now and reads the state.
    pub(super) fn refresh(&mut self) -> Result<MixerState, MixerError> {
        let mut present = HashSet::new();
        for direction in DIRECTIONS {
            // The enumerator failing is the audio system gone; see the
            // module's documentation.
            let devices = self
                .active(direction)
                .map_err(|_| MixerError::Disconnected)?;
            for device in devices {
                let Some(key) = endpoint::key(&device) else {
                    continue;
                };
                let changes = &self.changes;
                let opened = self
                    .endpoints
                    .get_or_try_insert(&key, || Endpoint::open(device, direction, changes));
                if or_skip(opened)?.is_some() {
                    present.insert(key);
                }
            }
        }
        self.endpoints.retain(|key, _| present.contains(key));

        let mut live = HashSet::new();
        for (_, device_id, endpoint) in self.endpoints.iter() {
            let device = DeviceId(device_id);
            let Some(controls) = or_skip(endpoint.sessions())? else {
                // Its sessions could not be listed this time: keep the ones
                // known, numbers and all, rather than renumber them next time.
                live.extend(
                    self.sessions
                        .iter()
                        .filter(|(_, _, session)| session.device == device)
                        .map(|(key, _, _)| key.to_owned()),
                );
                continue;
            };
            for control in controls {
                let Some(key) = session::key(&control) else {
                    continue;
                };
                let changes = &self.changes;
                let opened = self.sessions.get_or_try_insert(&key, || {
                    Session::open(&control, device, endpoint.direction, changes)
                });
                if or_skip(opened)?.is_some() {
                    live.insert(key);
                }
            }
        }
        self.sessions.retain(|key, _| live.contains(key));

        self.defaults = [
            self.default_key(Direction::Output)?,
            self.default_key(Direction::Input)?,
        ];
        self.state()
    }

    /// Reads every followed endpoint's and session's values.
    fn state(&self) -> Result<MixerState, MixerError> {
        let mut state = MixerState::default();
        for (key, id, endpoint) in self.endpoints.iter() {
            let default = self.default_of(endpoint.direction) == Some(key);
            if let Some(device) = or_skip(endpoint.device(DeviceId(id), key, default))? {
                state.devices.push(device);
            }
        }
        for (_, id, session) in self.sessions.iter() {
            if let Some(stream) = or_skip(session.stream(StreamId(id)))? {
                state.streams.push(stream);
            }
        }
        state.devices.sort_by_key(|device| device.id);
        state.streams.sort_by_key(|stream| stream.id);
        Ok(state)
    }

    fn default_of(&self, direction: Direction) -> Option<&str> {
        let index = usize::from(direction == Direction::Input);
        self.defaults[index].as_deref()
    }

    /// The volume control `target` names, as of the last read.
    fn control(&self, target: Target) -> Result<Control<'_>, MixerError> {
        let endpoint = |key: &str| {
            self.endpoints
                .iter()
                .find(|(candidate, _, _)| *candidate == key)
                .map(|(key, _, endpoint)| Control::Endpoint {
                    key,
                    volume: endpoint.volume(),
                })
        };
        match target {
            Target::Default(direction) => self.default_of(direction).and_then(endpoint),
            Target::Device(DeviceId(id)) => {
                self.endpoints
                    .by_id(id)
                    .map(|(key, endpoint)| Control::Endpoint {
                        key,
                        volume: endpoint.volume(),
                    })
            }
            Target::Stream(StreamId(id)) => self
                .sessions
                .by_id(id)
                .map(|(_, session)| Control::Session(session.volume())),
            // Windows reports no cards.
            Target::Card(_) => None,
        }
        .ok_or(MixerError::NoSuchTarget)
    }

    /// Makes `action`'s call on `target`. What Windows cannot do at all
    /// answers [`MixerError::Unsupported`] whatever the target, before the
    /// target is looked up.
    pub(super) fn apply(&self, target: Target, action: &Action) -> Result<(), MixerError> {
        match action {
            Action::MoveTo(_) => Err(MixerError::Unsupported(
                "Windows has no public API to move a stream to another device",
            )),
            Action::SetPort(_) => Err(MixerError::Unsupported("Windows reports no ports")),
            Action::SetProfile(_) => Err(MixerError::Unsupported(
                "Windows reports no cards or profiles",
            )),
            Action::SetMute(muted) => self.control(target)?.set_mute(*muted),
            Action::ToggleMute => {
                let control = self.control(target)?;
                control.set_mute(!control.muted()?)
            }
            Action::SetVolume(percent) => self
                .control(target)?
                .set_scalar(level::scalar_from_percent(*percent)),
            Action::Step(delta) => {
                let control = self.control(target)?;
                control.set_scalar(level::stepped_scalar(control.scalar()?, *delta))
            }
            Action::MakeDefault => match self.control(target)? {
                Control::Endpoint { key, .. } => policy_config::make_default(key),
                Control::Session(_) => Err(MixerError::Invalid("only a device can be the default")),
            },
        }
    }
}
