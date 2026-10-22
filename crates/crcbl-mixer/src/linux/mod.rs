//! Linux: the sound server, spoken to over its socket.
//!
//! PipeWire's own protocol when its socket is there. PulseAudio's native
//! protocol, the fallback for a system without PipeWire, is planned in
//! `docs/backlog.md` and not built: on such a system [`open`] reports
//! [`MixerError::NoServer`].

pub(crate) mod pipewire;

use crate::MixerError;

/// The backend this system's sound server is reached through.
pub(crate) type Backend = pipewire::PipeWire;

/// Connects to the sound server.
pub(crate) fn open(waker: Box<dyn Fn() + Send + Sync>) -> Result<Backend, MixerError> {
    if pipewire::PipeWire::available() {
        return pipewire::PipeWire::open(waker);
    }
    Err(MixerError::NoServer)
}
