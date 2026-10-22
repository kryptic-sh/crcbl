//! Control of the system's audio mixer: volume, mute, devices and streams.
//!
//! Not playback, which is `crcbl-audio`'s. This is what a desktop's volume
//! slider, output switcher and per-application mixer do: read the system's
//! output and input devices, the streams applications are playing and
//! recording, the sound cards' profiles and the devices' ports, and change
//! them. Any crcbl app can use it; lyr's bar is the first.
//!
//! # How each platform is reached
//!
//! Through what the system already runs and nothing a user would have to
//! install, so no command-line tool and no C client library:
//!
//! * **Linux:** the sound server's own protocol over its socket, written here.
//!   PipeWire's when it is running, PulseAudio's native protocol when it is
//!   not.
//! * **Windows:** WASAPI, the system's own audio API, through the `windows`
//!   crate. It reports no cards and no ports, cannot move a stream, and has no
//!   boost past 100 %.
//! * **macOS:** CoreAudio, planned and not built yet; [`Mixer::open`] reports
//!   [`MixerError::Unsupported`] there.
//!
//! # Changes without polling
//!
//! [`Mixer::open`] takes a waker, called from the mixer's own thread whenever
//! what [`Mixer::state`] would return has changed. An app's loop sleeps until
//! it is called, reads the new state, and draws. [`Mixer::set_interest`] limits
//! the calls to device changes, for an app that shows streams only now and
//! then.
//!
//! # Percent
//!
//! Volumes are percents on the scale every desktop mixer shows; see
//! [`level`].

pub mod level;
mod model;
#[cfg(any(target_os = "linux", windows))]
mod visible;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as platform;
#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", windows)))]
use unsupported as platform;

use std::fmt;

pub use model::{Card, CardId, Choice, Device, DeviceId, Direction, MixerState, Stream, StreamId};

/// What an [`Action`] is applied to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// The default device for a direction, whichever it is when the action
    /// arrives.
    Default(Direction),
    Device(DeviceId),
    Stream(StreamId),
    /// A sound card, for [`Action::SetProfile`].
    Card(CardId),
}

/// A change to the mixer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SetMute(bool),
    ToggleMute,
    /// Sets the volume to this percent, held to [`level::MAX_PERCENT`] on
    /// Linux and to [`level::SCALAR_MAX_PERCENT`] on Windows, which has no
    /// boost.
    SetVolume(u32),
    /// Moves the volume by this many percent, up or down, held to
    /// `0..=`[`level::MAX_PERCENT`] on Linux and to
    /// `0..=`[`level::SCALAR_MAX_PERCENT`] on Windows.
    Step(i32),
    /// Makes the target device the default for its direction.
    MakeDefault,
    /// Moves the target stream to this device.
    MoveTo(DeviceId),
    /// Switches the target device to the port of this [`Choice::name`].
    SetPort(String),
    /// Switches the target card to the profile of this [`Choice::name`].
    SetProfile(String),
}

/// Which changes wake an app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interest {
    /// Changes to devices, cards and defaults only: a stream starting,
    /// stopping or changing its volume does not wake.
    Devices,
    /// Every change, streams included. The default.
    DevicesAndStreams,
}

/// Why the mixer could not do what was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MixerError {
    /// No sound server this mixer can speak to is running.
    NoServer,
    /// This platform or server has no way to do this.
    Unsupported(&'static str),
    /// The target is not in the mixer's current state.
    NoSuchTarget,
    /// The action does not apply to the target, such as a port on a stream.
    Invalid(&'static str),
    /// The server's protocol went wrong: a malformed message, a refused
    /// client, or a socket that failed.
    Protocol(String),
    /// The sound server went away, as it does when it restarts; see
    /// [`Mixer::is_connected`].
    Disconnected,
}

impl fmt::Display for MixerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoServer => f.write_str("no sound server to control is running"),
            Self::Unsupported(what) => write!(f, "not supported here: {what}"),
            Self::NoSuchTarget => f.write_str("no such device, stream or card"),
            Self::Invalid(why) => write!(f, "that action does not apply: {why}"),
            Self::Protocol(what) => write!(f, "sound server protocol: {what}"),
            Self::Disconnected => f.write_str("the sound server went away"),
        }
    }
}

impl std::error::Error for MixerError {}

/// The system mixer, followed for as long as this lives.
#[derive(Debug)]
pub struct Mixer {
    backend: platform::Backend,
}

impl Mixer {
    /// Connects to the system mixer and reads its whole state.
    ///
    /// `waker` is called from the mixer's thread after each change, without
    /// arguments: read [`state`](Self::state) then. It must not block.
    ///
    /// # Errors
    ///
    /// [`MixerError::NoServer`] when no sound server it can speak to is
    /// running, [`MixerError::Unsupported`] on a platform it has no backend for
    /// yet, and [`MixerError::Protocol`] when the server could not be read.
    pub fn open(waker: impl Fn() + Send + Sync + 'static) -> Result<Self, MixerError> {
        platform::open(Box::new(waker)).map(|backend| Self { backend })
    }

    /// The mixer's state now.
    #[must_use]
    pub fn state(&self) -> MixerState {
        self.backend.state()
    }

    /// Whether the sound server is still there.
    ///
    /// False once it has gone, as it does when it restarts: the waker is
    /// called once more when that happens, [`state`](Self::state) keeps the
    /// last state it had, and [`apply`](Self::apply) answers
    /// [`MixerError::Disconnected`]. Open a new mixer to follow the new server,
    /// once it is up.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.backend.is_connected()
    }

    /// Which changes call the waker from now on.
    pub fn set_interest(&self, interest: Interest) {
        self.backend.set_interest(interest);
    }

    /// Sends `action` on `target` to the system. It is asynchronous: the
    /// change shows in [`state`](Self::state) once the server has made it, and
    /// the waker is called then.
    ///
    /// # Errors
    ///
    /// [`MixerError::NoSuchTarget`] when the target is not in the current
    /// state, [`MixerError::Invalid`] when the action does not apply to it,
    /// [`MixerError::Unsupported`] when the platform cannot do it,
    /// [`MixerError::Protocol`] when it could not be sent, and
    /// [`MixerError::Disconnected`] once the server has gone.
    pub fn apply(&self, target: Target, action: &Action) -> Result<(), MixerError> {
        self.backend.apply(target, action)
    }
}
