//! QOA ("Quite OK Audio") decoder — decodes the lossy compression format
//! described at <https://qoaformat.org> into the engine's [`AudioSample`] (f32).
//!
//! # What this handles
//!
//! Static QOA files (known total sample count), up to 8 channels, any sample
//! rate.  Output is interleaved f32, converted from the internal 16-bit
//! representation.
//!
//! # What this deliberately does not handle
//!
//! Streaming QOA (unknown total sample count, set to zero in the header).
//! The engine loads SFX fully into RAM; streaming decode is for music and
//! will use Vorbis/Opus behind a decoder seam.

use crate::AudioSample;

/// A decoded QOA file.
#[derive(Clone, Debug, PartialEq)]
pub struct QoaFile {
    /// Interleaved sample data, converted to f32.
    ///
    /// The QOA header counts samples *per channel*, so this is
    /// `header_samples × channels` long.
    pub samples: Vec<AudioSample>,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u8,
}

/// Parse errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QoaError {
    /// File is too short to contain a valid header.
    Truncated,
    /// Not a QOA file (wrong magic).
    MissingMagic,
    /// Zero samples declared in header (streaming, not supported).
    StreamingUnsupported,
    /// Channel count is zero.
    ZeroChannels,
    /// The declared sample count cannot be held by the bytes that follow it,
    /// or the interleaved total overflows a `usize`.
    SampleCountOverflow,
    /// Frame data truncated mid-stream.
    FrameTruncated,
    /// Frame size declared in header differs from computed.
    FrameSizeMismatch,
}

impl core::fmt::Display for QoaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated => f.write_str("QOA file truncated"),
            Self::MissingMagic => f.write_str("not a QOA file (expected 'qoaf' magic)"),
            Self::StreamingUnsupported => f.write_str("streaming QOA is not supported"),
            Self::ZeroChannels => f.write_str("QOA file declares zero channels"),
            Self::SampleCountOverflow => f.write_str("QOA sample count is not backed by the file"),
            Self::FrameTruncated => f.write_str("QOA frame data truncated"),
            Self::FrameSizeMismatch => f.write_str("QOA frame size mismatch"),
        }
    }
}

impl std::error::Error for QoaError {}

// ── Constants ────────────────────────────────────────────────────────────────

const SLICE_LEN: usize = 20;
/// Every slice is exactly eight bytes on the wire, whatever it decodes to.
const SLICE_BYTES: usize = 8;
const LMS_LEN: usize = 4;
const MAGIC: u32 = 0x716f_6166; // 'qoaf'

/// dequant_tab[sf][qr] = round(scalefactor_tab[sf] * dqt[qr])
/// with dqt = {0.75, -0.75, 2.5, -2.5, 4.5, -4.5, 7, -7}
const DEQUANT_TAB: [[i32; 8]; 16] = [
    [1, -1, 3, -3, 5, -5, 7, -7],
    [5, -5, 18, -18, 32, -32, 49, -49],
    [16, -16, 53, -53, 95, -95, 147, -147],
    [34, -34, 113, -113, 203, -203, 315, -315],
    [63, -63, 210, -210, 378, -378, 588, -588],
    [104, -104, 345, -345, 621, -621, 966, -966],
    [158, -158, 528, -528, 950, -950, 1477, -1477],
    [228, -228, 760, -760, 1368, -1368, 2128, -2128],
    [316, -316, 1053, -1053, 1895, -1895, 2947, -2947],
    [422, -422, 1405, -1405, 2529, -2529, 3934, -3934],
    [548, -548, 1828, -1828, 3290, -3290, 5117, -5117],
    [696, -696, 2320, -2320, 4176, -4176, 6496, -6496],
    [868, -868, 2893, -2893, 5207, -5207, 8099, -8099],
    [1064, -1064, 3548, -3548, 6386, -6386, 9933, -9933],
    [1286, -1286, 4288, -4288, 7718, -7718, 12005, -12005],
    [1536, -1536, 5120, -5120, 9216, -9216, 14336, -14336],
];

/// Reads a u32 in big-endian byte order.
fn read_u32_be(bytes: &[u8], pos: &mut usize) -> Result<u32, QoaError> {
    if *pos + 4 > bytes.len() {
        return Err(QoaError::Truncated);
    }
    let value = u32::from_be_bytes([
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
    ]);
    *pos += 4;
    Ok(value)
}

/// Reads a u16 in big-endian byte order.
fn read_u16_be(bytes: &[u8], pos: &mut usize) -> Result<u16, QoaError> {
    if *pos + 2 > bytes.len() {
        return Err(QoaError::Truncated);
    }
    let value = u16::from_be_bytes([bytes[*pos], bytes[*pos + 1]]);
    *pos += 2;
    Ok(value)
}

/// Reads a signed i16 in big-endian byte order.
fn read_i16_be(bytes: &[u8], pos: &mut usize) -> Result<i16, QoaError> {
    if *pos + 2 > bytes.len() {
        return Err(QoaError::Truncated);
    }
    let value = i16::from_be_bytes([bytes[*pos], bytes[*pos + 1]]);
    *pos += 2;
    Ok(value)
}

/// LMS predictor state for one channel.
#[derive(Clone, Copy, Debug)]
struct LmsState {
    history: [i32; LMS_LEN],
    weights: [i32; LMS_LEN],
}

impl LmsState {
    fn new() -> Self {
        Self {
            history: [0; LMS_LEN],
            weights: [0; LMS_LEN],
        }
    }
}

/// Decodes a QOA file from raw bytes.
///
/// Output is interleaved f32 samples at the file's sample rate.
pub fn decode(bytes: &[u8]) -> Result<QoaFile, QoaError> {
    let mut pos = 0;

    // ── File header ──────────────────────────────────────────────────────
    let magic = read_u32_be(bytes, &mut pos)?;
    if magic != MAGIC {
        return Err(QoaError::MissingMagic);
    }
    // The header counts samples *per channel*, not interleaved samples.
    let total_samples = read_u32_be(bytes, &mut pos)?;
    if total_samples == 0 {
        return Err(QoaError::StreamingUnsupported);
    }
    let total = total_samples as usize;

    // Reject a count the remaining bytes cannot possibly hold *before*
    // reserving for it. `total_samples` comes from the file, so an 8-byte file
    // declaring `u32::MAX` would otherwise ask for a 17 GB allocation and abort
    // the process in `handle_alloc_error`.
    //
    // An eight-byte slice decodes to `SLICE_LEN` samples of one channel, so
    // this bounds the *interleaved* length whatever the channel count is, and
    // each frame spends further bytes on its header and LMS state on top. With
    // at least one channel it also bounds the per-channel count.
    let max_interleaved = (bytes.len() - pos) / SLICE_BYTES * SLICE_LEN;
    if total > max_interleaved {
        return Err(QoaError::SampleCountOverflow);
    }

    // Group state across frames — channel count and sample rate must be
    // identical in every frame for static files.
    let mut channels: u8 = 0;
    let mut sample_rate: u32 = 0;
    let mut lms: Vec<LmsState> = Vec::new();
    let mut samples: Vec<AudioSample> = Vec::new();
    // Per-channel samples decoded so far. Comparing `samples.len()` against
    // `total` instead would stop a stereo file at half its length.
    let mut decoded_samples = 0usize;

    loop {
        if decoded_samples >= total {
            break;
        }
        // ── Frame header ─────────────────────────────────────────────────
        let num_channels = bytes.get(pos).copied().ok_or(QoaError::FrameTruncated)?;
        pos += 1;
        if num_channels == 0 {
            return Err(QoaError::ZeroChannels);
        }
        let sr = read_u24_be(bytes, &mut pos)?;
        let fsamples = read_u16_be(bytes, &mut pos)? as usize;
        let fsize = read_u16_be(bytes, &mut pos)? as usize;

        if channels == 0 {
            channels = num_channels;
            sample_rate = sr;
            lms.resize(channels as usize, LmsState::new());
            // Only now is the interleaved length known: the header count is
            // per channel. Check the product against the same byte budget, so
            // a file declaring many channels cannot multiply its way to an
            // allocation it has no data for.
            let interleaved = total
                .checked_mul(channels as usize)
                .filter(|n| *n <= max_interleaved)
                .ok_or(QoaError::SampleCountOverflow)?;
            samples.reserve_exact(interleaved);
        } else if num_channels != channels || sr != sample_rate {
            // Static files: every frame must have the same channels and rate.
            return Err(QoaError::FrameSizeMismatch);
        }

        // Validate frame size.
        let slices_in_frame = fsamples.div_ceil(SLICE_LEN);
        let expected_fsize =
            8 + LMS_LEN * 4 * num_channels as usize + 8 * slices_in_frame * num_channels as usize;
        if fsize != expected_fsize {
            return Err(QoaError::FrameSizeMismatch);
        }

        // ── LMS state per channel ────────────────────────────────────────
        #[allow(clippy::needless_range_loop)]
        for ch in 0..num_channels as usize {
            for i in 0..LMS_LEN {
                lms[ch].history[i] = read_i16_be(bytes, &mut pos)? as i32;
            }
            for i in 0..LMS_LEN {
                lms[ch].weights[i] = read_i16_be(bytes, &mut pos)? as i32;
            }
        }

        // ── Decode slices ────────────────────────────────────────────────
        let channel_samples = fsamples.min(total - decoded_samples);
        let mut decoded = vec![0i16; channel_samples * channels as usize];
        let full_slices = channel_samples / SLICE_LEN;
        let remainder = channel_samples % SLICE_LEN;

        for slice_idx in 0..full_slices {
            decode_full_slice(bytes, &mut pos, &mut lms, &mut decoded, slice_idx, channels)?;
        }
        if remainder > 0 {
            decode_partial_slice(
                bytes,
                &mut pos,
                &mut lms,
                &mut decoded,
                full_slices,
                remainder,
                channels,
            )?;
        }

        // Convert i16 to f32, interleaved.
        for &raw in &decoded {
            samples.push(f32::from(raw) / 32768.0);
        }
        decoded_samples += channel_samples;
    }

    Ok(QoaFile {
        samples,
        sample_rate,
        channels,
    })
}

fn decode_full_slice(
    bytes: &[u8],
    pos: &mut usize,
    lms: &mut [LmsState],
    decoded: &mut [i16],
    slice_idx: usize,
    channels: u8,
) -> Result<(), QoaError> {
    for ch in 0..channels as usize {
        decode_slice(bytes, pos, lms, decoded, slice_idx, ch, SLICE_LEN, channels)?;
    }
    Ok(())
}

fn decode_partial_slice(
    bytes: &[u8],
    pos: &mut usize,
    lms: &mut [LmsState],
    decoded: &mut [i16],
    slice_idx: usize,
    remainder: usize,
    channels: u8,
) -> Result<(), QoaError> {
    for ch in 0..channels as usize {
        decode_slice(bytes, pos, lms, decoded, slice_idx, ch, remainder, channels)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decode_slice(
    bytes: &[u8],
    pos: &mut usize,
    lms: &mut [LmsState],
    decoded: &mut [i16],
    slice_idx: usize,
    ch: usize,
    count: usize,
    channels: u8,
) -> Result<(), QoaError> {
    if *pos + 8 > bytes.len() {
        return Err(QoaError::FrameTruncated);
    }
    let slice: [u8; 8] = bytes[*pos..*pos + 8].try_into().unwrap();
    *pos += 8;

    let sf_quant = (slice[0] >> 4) as usize;
    let dequant_row = &DEQUANT_TAB[sf_quant];

    // Extract 20 × 3-bit quantized residuals from 8 bytes.
    // sf_quant occupies bits 63-60; qr bits fill 59-0 (20 × 3 = 60 bits).
    let raw = u64::from_be_bytes(slice);
    let mut qr = [0u8; SLICE_LEN];
    #[allow(clippy::needless_range_loop)]
    for i in 0..SLICE_LEN {
        qr[i] = ((raw >> (57 - i * 3)) & 0x07) as u8;
    }

    // Decode `count` samples.
    //
    // The arithmetic is `wrapping_*` throughout to match the reference
    // decoder's C `int`, which wraps in practice. A crafted file can drive the
    // weights far outside the range real audio produces, and a debug build
    // would otherwise panic on the overflow rather than decoding garbage.
    let st = &mut lms[ch];
    for n in 0..count {
        let residual = dequant_row[qr[n] as usize];
        // LMS prediction: dot product, shifted.
        let mut prediction = 0i32;
        for i in 0..LMS_LEN {
            prediction = prediction.wrapping_add(st.history[i].wrapping_mul(st.weights[i]));
        }
        let prediction = prediction >> 13;
        let sample = prediction.saturating_add(residual).clamp(-32768, 32767) as i16;
        decoded[(slice_idx * SLICE_LEN + n) * channels as usize + ch] = sample;

        // Update LMS weights, per `qoa.h`:
        //
        //     weights[i] += history[i] < 0 ? -delta : delta;
        //
        // The *sign* comes from the history, the *magnitude* from the delta.
        // Taking them the other way round makes the predictor diverge from the
        // encoder's, so anything but silence decodes to noise — and it needed a
        // non-spec weight clamp to stop it exploding, which is why there is no
        // clamp here: the reference has none.
        let delta = residual >> 4;
        for i in 0..LMS_LEN {
            let signed = if st.history[i] < 0 { -delta } else { delta };
            st.weights[i] = st.weights[i].wrapping_add(signed);
        }
        // Shift history.
        st.history[0] = st.history[1];
        st.history[1] = st.history[2];
        st.history[2] = st.history[3];
        st.history[3] = sample as i32;
    }

    Ok(())
}

/// Reads a 24-bit unsigned integer in big-endian byte order.
fn read_u24_be(bytes: &[u8], pos: &mut usize) -> Result<u32, QoaError> {
    if *pos + 3 > bytes.len() {
        return Err(QoaError::Truncated);
    }
    let value = u32::from_be_bytes([0, bytes[*pos], bytes[*pos + 1], bytes[*pos + 2]]);
    *pos += 3;
    Ok(value)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_without_the_qoa_magic_is_rejected_as_missing_magic() {
        assert_eq!(decode(b"not a qoa file"), Err(QoaError::MissingMagic));
    }

    #[test]
    fn a_qoa_header_shorter_than_its_magic_is_rejected_as_truncated() {
        assert_eq!(decode(&[0x71, 0x6f, 0x61]), Err(QoaError::Truncated));
    }

    #[test]
    fn a_zero_sample_count_is_read_as_streaming_and_refused() {
        let mut header = vec![0x71, 0x6f, 0x61, 0x66]; // 'qoaf'
        header.extend_from_slice(&0u32.to_be_bytes()); // zero samples = streaming
        assert_eq!(decode(&header), Err(QoaError::StreamingUnsupported));
    }

    /// Builds a minimal valid QOA file with the given audio properties.
    /// The LMS state is zeroed and slices are silent (scalefactor 0, all qr=0).
    fn silent_qoa(channels: u8, sample_rate: u32, sample_count: u32) -> Vec<u8> {
        const FRAME_LEN: usize = 256 * 20; // slices_per_frame * slice_len
        let mut out = Vec::new();
        // File header: magic + total samples.
        out.extend_from_slice(b"qoaf");
        out.extend_from_slice(&sample_count.to_be_bytes());

        let frames = (sample_count as usize).div_ceil(FRAME_LEN);
        for fi in 0..frames {
            let remaining = sample_count as usize - fi * FRAME_LEN;
            let fsamples = remaining.min(FRAME_LEN);
            let slices = fsamples.div_ceil(SLICE_LEN);
            let fsize = 8 + LMS_LEN * 4 * channels as usize + 8 * slices * channels as usize;

            // Frame header.
            out.push(channels);
            // 24-bit sample rate.
            out.push((sample_rate >> 16) as u8);
            out.push((sample_rate >> 8) as u8);
            out.push(sample_rate as u8);
            out.extend_from_slice(&(fsamples as u16).to_be_bytes());
            out.extend_from_slice(&(fsize as u16).to_be_bytes());

            // LMS state per channel: zeroed.
            for _ch in 0..channels {
                for _ in 0..(LMS_LEN * 2) {
                    // history + weights = 8 i16 values
                    out.extend_from_slice(&0i16.to_be_bytes());
                }
            }

            // Slices: all silent (sf_quant=0, all qr=0 → 8 zero bytes each).
            for _s in 0..slices {
                for _ch in 0..channels {
                    out.extend_from_slice(&[0u8; 8]);
                }
            }
        }
        out
    }

    #[test]
    fn a_silent_mono_qoa_decodes_to_its_declared_rate_length_and_silence() {
        let data = silent_qoa(1, 44100, 40);
        let qoa = decode(&data).unwrap();
        assert_eq!(qoa.channels, 1);
        assert_eq!(qoa.sample_rate, 44100);
        assert_eq!(qoa.samples.len(), 40);
        for &s in &qoa.samples {
            assert!((s - 0.0).abs() < 0.001, "silent sample is non-zero: {s}");
        }
    }

    #[test]
    fn a_silent_stereo_qoa_decodes_to_two_interleaved_samples_per_frame() {
        let data = silent_qoa(2, 48000, 60);
        let qoa = decode(&data).unwrap();
        assert_eq!(qoa.channels, 2);
        assert_eq!(qoa.sample_rate, 48000);
        assert_eq!(qoa.samples.len(), 120); // 60 samples * 2 channels
        for &s in &qoa.samples {
            assert!((s - 0.0).abs() < 0.001);
        }
    }

    /// Builds a one-frame, one-slice mono QOA file with an explicit LMS state.
    ///
    /// [`silent_qoa`] can only produce all-zero LMS state and all-zero
    /// residuals, where the weight update is a no-op — which is exactly why an
    /// inverted update was invisible to every test.
    fn qoa_with_lms(
        sample_count: u16,
        history: [i16; LMS_LEN],
        weights: [i16; LMS_LEN],
        sf_quant: u8,
        qr: [u8; SLICE_LEN],
    ) -> Vec<u8> {
        assert!(sample_count as usize <= SLICE_LEN, "one slice only");
        let mut out = b"qoaf".to_vec();
        out.extend_from_slice(&u32::from(sample_count).to_be_bytes());

        let fsize = 8 + LMS_LEN * 4 + SLICE_BYTES;
        out.push(1); // channels
        out.extend_from_slice(&[0x00, 0xAC, 0x44]); // 44100 Hz as a u24
        out.extend_from_slice(&sample_count.to_be_bytes());
        out.extend_from_slice(&(fsize as u16).to_be_bytes());
        for h in history {
            out.extend_from_slice(&h.to_be_bytes());
        }
        for w in weights {
            out.extend_from_slice(&w.to_be_bytes());
        }

        // sf_quant in bits 63..60, then 20 × 3-bit residual indices.
        let mut raw = u64::from(sf_quant) << 60;
        for (i, q) in qr.iter().enumerate() {
            raw |= u64::from(*q) << (57 - i * 3);
        }
        out.extend_from_slice(&raw.to_be_bytes());
        out
    }

    #[test]
    fn lms_weight_update_takes_its_sign_from_the_history() {
        // Hand-computed against `qoa.h`, which does
        // `weights[i] += history[i] < 0 ? -delta : delta`:
        //
        //   start: history = [0, 0, 0, -1000], weights = [0, 0, 0, 8192]
        //   n=0  prediction = (-1000 * 8192) >> 13           = -1000
        //        residual   = DEQUANT_TAB[15][4]             =  9216
        //        sample     = -1000 + 9216                   =  8216
        //        delta      = 9216 >> 4                      =   576
        //        weights    = [576, 576, 576, 8192 - 576]    → last one negated
        //        history    = [0, 0, -1000, 8216]
        //   n=1  prediction = (-1000*576 + 8216*7616) >> 13  =  7568
        //        residual   = DEQUANT_TAB[15][0]             =  1536
        //        sample     = 7568 + 1536                    =  9104
        //
        // Taking the sign from `delta` and the magnitude from the history —
        // the inverted form — gives 8749 for the second sample.
        let mut qr = [0u8; SLICE_LEN];
        qr[0] = 4;
        let data = qoa_with_lms(2, [0, 0, 0, -1000], [0, 0, 0, 8192], 15, qr);

        let qoa = decode(&data).unwrap();
        assert_eq!(qoa.samples.len(), 2);
        assert_eq!(qoa.samples[0], 8216.0 / 32768.0);
        assert_eq!(qoa.samples[1], 9104.0 / 32768.0);
    }

    #[test]
    fn a_sound_spanning_more_than_one_qoa_frame_decodes_to_every_sample() {
        // 520 samples = 2 frames (256 + 264)
        let data = silent_qoa(1, 44100, 520);
        let qoa = decode(&data).unwrap();
        assert_eq!(qoa.samples.len(), 520);
    }

    #[test]
    fn decode_multi_frame_stereo_keeps_both_channels() {
        // The header count is per channel, so 6000 declared samples over two
        // frames of a stereo file is 12000 interleaved. Comparing the header
        // count against the interleaved length stopped at half the file.
        let data = silent_qoa(2, 48_000, 6000);
        let qoa = decode(&data).unwrap();
        assert_eq!(qoa.channels, 2);
        assert_eq!(qoa.samples.len(), 12_000);
    }

    #[test]
    fn absurd_sample_count_rejected_without_allocating() {
        // Eight bytes declaring u32::MAX samples: fed straight into
        // `Vec::with_capacity` that is a 17 GB reservation and an abort.
        let mut header = b"qoaf".to_vec();
        header.extend_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(decode(&header), Err(QoaError::SampleCountOverflow));
    }

    #[test]
    fn sample_count_beyond_the_remaining_bytes_rejected() {
        // A well-formed single-frame file whose header claims far more samples
        // than the frames that follow could hold.
        let mut data = silent_qoa(1, 44100, 40);
        data[4..8].copy_from_slice(&100_000u32.to_be_bytes());
        assert_eq!(decode(&data), Err(QoaError::SampleCountOverflow));
    }

    #[test]
    fn decode_last_frame_partial_slice() {
        // 30 samples: 1 frame, but only 1 full slice (20) + 1 partial (10).
        let data = silent_qoa(1, 44100, 30);
        let qoa = decode(&data).unwrap();
        assert_eq!(qoa.samples.len(), 30);
    }

    #[test]
    fn a_qoa_header_declaring_no_channels_is_rejected_rather_than_decoded() {
        let data = silent_qoa(0, 44100, 20);
        assert_eq!(decode(&data), Err(QoaError::ZeroChannels));
    }
}
