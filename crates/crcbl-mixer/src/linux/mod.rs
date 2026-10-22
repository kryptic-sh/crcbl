//! Linux: the sound server, spoken to over its socket.
//!
//! PipeWire's own protocol when its socket is there; PulseAudio's native
//! protocol when it is not. `CRCBL_MIXER_BACKEND` set to `pipewire` or `pulse`
//! picks one regardless, which is how the PulseAudio backend is tested on a
//! PipeWire machine (`pipewire-pulse` serves its protocol).

pub(crate) mod pipewire;
mod pulse;

use crate::{Action, Interest, MixerError, MixerState, Target};

/// The sound server this mixer is connected to.
#[derive(Debug)]
pub(crate) enum Backend {
    PipeWire(pipewire::PipeWire),
    Pulse(pulse::Pulse),
}

/// Connects to the sound server: PipeWire's if it is running, else
/// PulseAudio's.
pub(crate) fn open(waker: Box<dyn Fn() + Send + Sync>) -> Result<Backend, MixerError> {
    match std::env::var("CRCBL_MIXER_BACKEND").as_deref() {
        Ok("pipewire") => return pipewire::PipeWire::open(waker).map(Backend::PipeWire),
        Ok("pulse") => return pulse::Pulse::open(waker).map(Backend::Pulse),
        _ => {}
    }
    if pipewire::PipeWire::available() {
        return pipewire::PipeWire::open(waker).map(Backend::PipeWire);
    }
    if pulse::Pulse::available() {
        return pulse::Pulse::open(waker).map(Backend::Pulse);
    }
    Err(MixerError::NoServer)
}

impl Backend {
    pub(crate) fn state(&self) -> MixerState {
        match self {
            Self::PipeWire(backend) => backend.state(),
            Self::Pulse(backend) => backend.state(),
        }
    }

    pub(crate) fn is_connected(&self) -> bool {
        match self {
            Self::PipeWire(backend) => backend.is_connected(),
            Self::Pulse(backend) => backend.is_connected(),
        }
    }

    pub(crate) fn set_interest(&self, interest: Interest) {
        match self {
            Self::PipeWire(backend) => backend.set_interest(interest),
            Self::Pulse(backend) => backend.set_interest(interest),
        }
    }

    pub(crate) fn apply(&self, target: Target, action: &Action) -> Result<(), MixerError> {
        match self {
            Self::PipeWire(backend) => backend.apply(target, action),
            Self::Pulse(backend) => backend.apply(target, action),
        }
    }
}
