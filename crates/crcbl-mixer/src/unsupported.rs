//! A platform the mixer has no backend for yet: Windows and macOS until
//! theirs land (`docs/backlog.md`, "System audio control").
//!
//! [`Backend`] has no values, so a [`Mixer`](crate::Mixer) cannot exist here:
//! [`open`] is the one function that answers, and it answers
//! [`MixerError::Unsupported`]. Every other method is a `match` on nothing.

use crate::{Action, Interest, MixerError, MixerState, Target};

/// No backend: uninhabited.
#[derive(Debug)]
pub(crate) enum Backend {}

pub(crate) fn open(_waker: Box<dyn Fn() + Send + Sync>) -> Result<Backend, MixerError> {
    Err(MixerError::Unsupported(
        "the mixer has a Linux backend only so far; Windows and macOS are planned",
    ))
}

impl Backend {
    pub(crate) fn state(&self) -> MixerState {
        match *self {}
    }

    pub(crate) fn set_interest(&self, _interest: Interest) {
        match *self {}
    }

    pub(crate) fn apply(&self, _target: Target, _action: &Action) -> Result<(), MixerError> {
        match *self {}
    }
}
