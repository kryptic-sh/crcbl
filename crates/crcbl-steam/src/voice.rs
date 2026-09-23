//! `ISteamUser` voice: the microphone as compressed packets, and packets as
//! PCM for the game's own mixer.
//!
//! Steam records only while the game says so and plays nothing back through
//! this API: [`VoiceCapture`] is push-to-talk under the game's control, its
//! packets travel over the game's own transport, and each receiver turns them
//! into mono `f32` samples with [`Voice::decompress`] (`docs/plan/42-steam.md`,
//! "Voice").

use std::{marker::PhantomData, rc::Rc, sync::Arc};

use crate::{Steam, client::Client};

/// The rate [`Voice::decompress`] is asked for by a game feeding
/// `crcbl-audio`: its `INTERNAL_SAMPLE_RATE`, which the umbrella crate's
/// tests pin this to.
pub const VOICE_SAMPLE_RATE: u32 = 48_000;

/// The lowest rate the decoder supports (`isteamuser.h`, `DecompressVoice`).
pub const MIN_VOICE_SAMPLE_RATE: u32 = 11_025;

/// The highest rate the decoder supports (`isteamuser.h`, `DecompressVoice`).
pub const MAX_VOICE_SAMPLE_RATE: u32 = 48_000;

/// The first buffer [`Voice::decompress`] offers: `isteamuser.h` suggests
/// starting "with a 20kb buffer".
const DECOMPRESS_START_BYTES: usize = 20 * 1024;

/// The most one packet may grow [`VoiceCapture::poll`]'s buffer, or
/// [`Voice::decompress`]'s, to — far past the buffer `isteamuser.h` suggests
/// for a whole decoded packet. A library that asks for more is broken, or was
/// handed a packet built to make it ask, and the call says so rather than
/// allocate.
const MAX_PACKET_BYTES: usize = 1024 * 1024;

/// `EVoiceResult`'s values (`steamclientpublic.h`).
mod result {
    pub(super) const OK: i32 = 0;
    pub(super) const NOT_INITIALIZED: i32 = 1;
    pub(super) const NOT_RECORDING: i32 = 2;
    pub(super) const NO_DATA: i32 = 3;
    pub(super) const BUFFER_TOO_SMALL: i32 = 4;
    pub(super) const DATA_CORRUPTED: i32 = 5;
    pub(super) const RESTRICTED: i32 = 6;
    pub(super) const UNSUPPORTED_CODEC: i32 = 7;
    pub(super) const RECEIVER_OUT_OF_DATE: i32 = 8;
    pub(super) const RECEIVER_DID_NOT_ANSWER: i32 = 9;
}

/// Why a voice call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VoiceError {
    /// Steam's voice system is not initialised
    /// (`k_EVoiceResultNotInitialized`).
    #[error("Steam voice is not initialised")]
    NotInitialized,
    /// The account may not use voice chat (`k_EVoiceResultRestricted`) — a
    /// limited or parentally restricted account.
    #[error("this Steam account may not use voice chat")]
    Restricted,
    /// The packet could not be decoded (`k_EVoiceResultDataCorrupted`).
    #[error("the voice packet is corrupt")]
    DataCorrupted,
    /// The packet uses a codec this client lacks
    /// (`k_EVoiceResultUnsupportedCodec`).
    #[error("the voice packet uses a codec this Steam client does not support")]
    UnsupportedCodec,
    /// `k_EVoiceResultReceiverOutOfDate`.
    #[error("the receiving Steam client is out of date")]
    ReceiverOutOfDate,
    /// `k_EVoiceResultReceiverDidNotAnswer`.
    #[error("the receiving Steam client did not answer")]
    ReceiverDidNotAnswer,
    /// Steam kept answering `k_EVoiceResultBufferTooSmall` past the size it
    /// asked for, or past the most one packet may be.
    #[error("Steam asked for a buffer of {needed} bytes and still found it too small")]
    BufferTooSmall {
        /// The last size tried.
        needed: usize,
    },
    /// A decoded buffer with a byte left over: not 16-bit samples.
    #[error("Steam decoded {0} bytes, which is not whole 16-bit samples")]
    OddLength(usize),
    /// A rate outside [`MIN_VOICE_SAMPLE_RATE`]`..=`[`MAX_VOICE_SAMPLE_RATE`].
    #[error(
        "the voice decoder takes {min} to {max} Hz, not {0}",
        min = MIN_VOICE_SAMPLE_RATE,
        max = MAX_VOICE_SAMPLE_RATE
    )]
    SampleRate(u32),
    /// A second [`VoiceCapture`] while one is alive: two would start and stop
    /// the one microphone against each other.
    #[error("a VoiceCapture is already open")]
    AlreadyCapturing,
    /// An `EVoiceResult` this crate does not name.
    #[error("EVoiceResult {0}")]
    Other(i32),
}

impl VoiceError {
    /// The error an `EVoiceResult` that is not `OK`, `NoData`,
    /// `NotRecording` or `BufferTooSmall` means.
    const fn from_result(result: i32) -> Self {
        match result {
            result::NOT_INITIALIZED => Self::NotInitialized,
            result::RESTRICTED => Self::Restricted,
            result::DATA_CORRUPTED => Self::DataCorrupted,
            result::UNSUPPORTED_CODEC => Self::UnsupportedCodec,
            result::RECEIVER_OUT_OF_DATE => Self::ReceiverOutOfDate,
            result::RECEIVER_DID_NOT_ANSWER => Self::ReceiverDidNotAnswer,
            other => Self::Other(other),
        }
    }
}

/// `ISteamUser`'s voice calls, borrowed from a [`Steam`]; from
/// [`Steam::voice`].
#[derive(Debug, Clone, Copy)]
pub struct Voice<'a> {
    steam: &'a Steam,
}

impl Steam {
    /// Voice capture and decoding.
    #[must_use]
    pub fn voice(&self) -> Voice<'_> {
        Voice { steam: self }
    }
}

impl Voice<'_> {
    /// The microphone, idle until [`VoiceCapture::set_transmitting`].
    ///
    /// # Errors
    ///
    /// [`VoiceError::AlreadyCapturing`] while another capture is alive.
    pub fn capture(&self) -> Result<VoiceCapture, VoiceError> {
        if self.steam.voice_capture.borrow().upgrade().is_some() {
            return Err(VoiceError::AlreadyCapturing);
        }
        let alive = Rc::new(());
        *self.steam.voice_capture.borrow_mut() = Rc::downgrade(&alive);
        Ok(VoiceCapture {
            client: Arc::clone(&self.steam.client),
            transmitting: false,
            recording: false,
            buffer: Vec::new(),
            _alive: alive,
            _not_send: PhantomData,
        })
    }

    /// Decodes one packet from any player's [`VoiceCapture`] into mono
    /// samples in `[-1.0, 1.0)` at `sample_rate` (`DecompressVoice`) — pass
    /// [`VOICE_SAMPLE_RATE`] for `crcbl-audio`.
    ///
    /// # Errors
    ///
    /// [`VoiceError::SampleRate`] outside the decoder's range, before any
    /// call; otherwise what Steam answers — a corrupt packet, a codec this
    /// client lacks — or [`VoiceError::BufferTooSmall`] when the size Steam
    /// asked for was still too small, or past what one packet may decode to.
    pub fn decompress(&self, compressed: &[u8], sample_rate: u32) -> Result<Vec<f32>, VoiceError> {
        if !(MIN_VOICE_SAMPLE_RATE..=MAX_VOICE_SAMPLE_RATE).contains(&sample_rate) {
            return Err(VoiceError::SampleRate(sample_rate));
        }
        let Ok(compressed_len) = u32::try_from(compressed.len()) else {
            return Err(VoiceError::DataCorrupted);
        };
        let client = &self.steam.client;
        let mut out = vec![0_u8; DECOMPRESS_START_BYTES];
        // At most twice: the first answer names the size a second one needs.
        for _ in 0..2 {
            let capacity = u32::try_from(out.len()).unwrap_or(u32::MAX);
            let mut written = 0_u32;
            // SAFETY: `client.user` is the non-null interface init resolved,
            // `Steam` is `!Send` so this is the pump thread, `compressed` is
            // `compressed_len` readable bytes, `out` is `capacity` writable
            // bytes, and `written` is writable.
            let answer = unsafe {
                (client.lib.fns.user.decompress_voice)(
                    client.user,
                    compressed.as_ptr().cast(),
                    compressed_len,
                    out.as_mut_ptr().cast(),
                    capacity,
                    &raw mut written,
                    sample_rate,
                )
            };
            let written = usize::try_from(written).unwrap_or(usize::MAX);
            match answer {
                result::OK if written <= out.len() => return samples(&out[..written]),
                result::OK => return Err(VoiceError::BufferTooSmall { needed: written }),
                result::BUFFER_TOO_SMALL if written > MAX_PACKET_BYTES => {
                    return Err(VoiceError::BufferTooSmall { needed: written });
                }
                result::BUFFER_TOO_SMALL if written > out.len() => out.resize(written, 0),
                result::BUFFER_TOO_SMALL => {
                    return Err(VoiceError::BufferTooSmall { needed: out.len() });
                }
                other => return Err(VoiceError::from_result(other)),
            }
        }
        Err(VoiceError::BufferTooSmall { needed: out.len() })
    }

    /// The decoder's native rate (`GetVoiceOptimalSampleRate`): the cheapest
    /// to decode at, not necessarily the best to play.
    #[must_use]
    pub fn optimal_sample_rate(&self) -> u32 {
        let client = &self.steam.client;
        // SAFETY: as in `decompress`.
        unsafe { (client.lib.fns.user.get_voice_optimal_sample_rate)(client.user) }
    }
}

/// Little-endian 16-bit samples as `f32`: `i16::MIN` is `-1.0` and
/// `i16::MAX` just under `1.0`.
fn samples(bytes: &[u8]) -> Result<Vec<f32>, VoiceError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(VoiceError::OddLength(bytes.len()));
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0)
        .collect())
}

/// The local microphone, recording while the game transmits
/// (`StartVoiceRecording` / `StopVoiceRecording`).
///
/// Push-to-talk has a tail: after [`set_transmitting(false)`](Self::set_transmitting)
/// Steam keeps recording briefly, so [`poll`](Self::poll) goes on handing out
/// packets until Steam says it has stopped. Dropping the capture stops
/// recording. One at a time, `!Send`: it calls Steam on the pump thread.
#[derive(Debug)]
pub struct VoiceCapture {
    client: Arc<Client>,
    /// What the game last asked for.
    transmitting: bool,
    /// Whether Steam may still be recording: from a start until a poll sees
    /// `NotRecording` after a stop.
    recording: bool,
    buffer: Vec<u8>,
    /// Held while this capture lives; [`Voice::capture`] refuses a second.
    _alive: Rc<()>,
    _not_send: PhantomData<*const ()>,
}

impl VoiceCapture {
    /// The game's push-to-talk: `true` starts recording and `false` stops
    /// it, each only on a change.
    pub fn set_transmitting(&mut self, on: bool) {
        if on == self.transmitting {
            return;
        }
        let client = &self.client;
        // SAFETY: `client.user` is the non-null interface; `VoiceCapture` is
        // `!Send`, so this is the pump thread it was made on.
        unsafe {
            if on {
                (client.lib.fns.user.start_voice_recording)(client.user);
            } else {
                (client.lib.fns.user.stop_voice_recording)(client.user);
            }
        }
        self.transmitting = on;
        if on {
            self.recording = true;
        }
    }

    /// Whether Steam may still be recording: while transmitting, and after
    /// it until a poll sees the tail end.
    #[must_use]
    pub const fn recording(&self) -> bool {
        self.recording
    }

    /// The next compressed packet, if Steam has one (`GetAvailableVoice`,
    /// then `GetVoice` with the deprecated uncompressed arguments off). Call
    /// every frame, and send what it answers as it is — any Steam client can
    /// decode it.
    ///
    /// # Errors
    ///
    /// What Steam answers — [`VoiceError::Restricted`] for an account that may
    /// not use voice, say — or [`VoiceError::BufferTooSmall`] when Steam
    /// wants more than a packet can be.
    pub fn poll(&mut self) -> Result<Option<Vec<u8>>, VoiceError> {
        if !self.recording {
            return Ok(None);
        }
        let client = Arc::clone(&self.client);
        let user = &client.lib.fns.user;
        let mut available = 0_u32;
        // SAFETY: as in `set_transmitting`; the deprecated uncompressed size
        // is not asked for (null, rate zero).
        let answer = unsafe {
            (user.get_available_voice)(client.user, &raw mut available, core::ptr::null_mut(), 0)
        };
        match answer {
            result::OK => {}
            result::NO_DATA => return Ok(None),
            result::NOT_RECORDING => return Ok(self.not_recording()),
            other => return Err(VoiceError::from_result(other)),
        }
        let mut size = usize::try_from(available).unwrap_or(usize::MAX).max(1);
        loop {
            if size > MAX_PACKET_BYTES {
                return Err(VoiceError::BufferTooSmall { needed: size });
            }
            if self.buffer.len() < size {
                self.buffer.resize(size, 0);
            }
            let capacity = u32::try_from(self.buffer.len()).unwrap_or(u32::MAX);
            let mut written = 0_u32;
            // SAFETY: as above; `buffer` is `capacity` writable bytes and
            // `written` is writable; the five deprecated uncompressed
            // arguments are false, null and zero.
            let answer = unsafe {
                (user.get_voice)(
                    client.user,
                    true,
                    self.buffer.as_mut_ptr().cast(),
                    capacity,
                    &raw mut written,
                    false,
                    core::ptr::null_mut(),
                    0,
                    core::ptr::null_mut(),
                    0,
                )
            };
            match answer {
                result::OK => {
                    let written = usize::try_from(written).unwrap_or(usize::MAX);
                    let Some(packet) = self.buffer.get(..written) else {
                        return Err(VoiceError::BufferTooSmall { needed: written });
                    };
                    return Ok(Some(packet.to_vec()));
                }
                result::BUFFER_TOO_SMALL => size = self.buffer.len().saturating_mul(2),
                result::NO_DATA => return Ok(None),
                result::NOT_RECORDING => return Ok(self.not_recording()),
                other => return Err(VoiceError::from_result(other)),
            }
        }
    }

    /// Steam reports it is not recording: after a stop, the tail has ended;
    /// while transmitting, the start has not taken hold yet, so keep polling.
    fn not_recording(&mut self) -> Option<Vec<u8>> {
        if !self.transmitting {
            self.recording = false;
        }
        None
    }
}

impl Drop for VoiceCapture {
    fn drop(&mut self) {
        if self.transmitting {
            // SAFETY: as in `set_transmitting`.
            unsafe { (self.client.lib.fns.user.stop_voice_recording)(self.client.user) };
        }
    }
}

#[cfg(test)]
mod tests;
