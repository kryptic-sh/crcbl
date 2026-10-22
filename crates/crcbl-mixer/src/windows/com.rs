//! COM itself: the apartment the mixer's thread joins, the strings COM hands
//! over, and what a failed call means to the mixer.

use std::marker::PhantomData;

use windows::Win32::Media::Audio::{AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_SERVICE_NOT_RUNNING};
use windows::Win32::System::Com::{
    COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
};
use windows::core::PWSTR;

use crate::MixerError;

/// The calling thread's membership of COM's multithreaded apartment, left
/// when this is dropped.
///
/// **Not `Send`**, through the raw pointer it holds no value of: COM's
/// initialisation belongs to a thread, so the guard has to drop on the thread
/// that made it.
#[derive(Debug)]
pub(super) struct Apartment(PhantomData<*const ()>);

impl Apartment {
    /// Joins the calling thread to the multithreaded apartment, which every
    /// audio object the mixer makes lives in. Windows calls the mixer's
    /// callbacks on threads of its own in the same apartment, so nothing has
    /// to be marshalled between them.
    pub(super) fn join() -> Result<Self, MixerError> {
        // SAFETY: no reserved pointer. A success, including `S_FALSE` for a
        // thread already in this apartment, is balanced by the one
        // `CoUninitialize` in `drop`, on this thread since the guard is not
        // `Send`.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .map_err(|error| failed("joining COM's multithreaded apartment", &error))?;
        Ok(Self(PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful `CoInitializeEx` in `join`, on the
        // same thread. Every COM object the thread made is released before
        // this guard drops (`Audio` is dropped first by `run`).
        unsafe { CoUninitialize() }
    }
}

/// A string COM allocated for the caller, read and then freed.
pub(super) fn take(text: PWSTR) -> Option<String> {
    if text.is_null() {
        return None;
    }
    // SAFETY: COM hands over a NUL-terminated UTF-16 string the caller owns;
    // it is read once, then freed once with the allocator COM made it with,
    // and not touched again.
    unsafe {
        let read = text.to_string().ok();
        CoTaskMemFree(Some(text.0.cast_const().cast()));
        read
    }
}

/// Whether a failed call says the Windows audio service is not running: it
/// was stopped, or is restarting.
pub(super) fn is_gone(error: &windows::core::Error) -> bool {
    error.code() == AUDCLNT_E_SERVICE_NOT_RUNNING
}

/// What a failed call doing `what` means to the mixer.
///
/// [`MixerError::Disconnected`] when the audio service is not running, which
/// the mixer's thread takes as the service gone (and [`super::open`] answers
/// as [`MixerError::NoServer`]); [`MixerError::NoSuchTarget`] when the
/// endpoint was removed under the call (`AUDCLNT_E_DEVICE_INVALIDATED`); else
/// [`MixerError::Protocol`] with the HRESULT and what failed.
pub(super) fn failed(what: &str, error: &windows::core::Error) -> MixerError {
    if is_gone(error) {
        MixerError::Disconnected
    } else if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        MixerError::NoSuchTarget
    } else {
        MixerError::Protocol(format!("WASAPI, {what}: {error}"))
    }
}
