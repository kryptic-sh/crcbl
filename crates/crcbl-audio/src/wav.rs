//! WAV (RIFF) PCM decoder — reads interleaved integer samples and converts
//! them to the engine's [`AudioSample`] (f32).
//!
//! # What this handles
//!
//! Uncompressed PCM in the canonical WAV container: integer samples at any
//! supported bit depth (8/16/24/32, signed and unsigned), any channel count,
//! any sample rate.  IEEE float WAV (`format = 3`) is also decoded.
//!
//! # What this deliberately does not handle
//!
//! Compressed WAV (μ-law, ADPCM, MP3-in-WAV — formats other than 1 and 3).
//! Multi-`data` chunks, `fact` chunks, or extended `WAVEFORMATEXTENSIBLE` with
//! channel masks: those carry no information the engine uses.  Seeking within a
//! stream is not needed for SFX (files are fully resident).

use crate::AudioSample;

/// Everything a WAV file carries that the engine cares about.
#[derive(Clone, Debug, PartialEq)]
pub struct WavFile {
    /// Interleaved sample data, converted to f32.
    pub samples: Vec<AudioSample>,
    /// Original sample rate.
    pub sample_rate: u32,
    /// Original channel count.
    pub channels: u16,
}

/// Parse errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WavError {
    /// The file is shorter than a valid header.
    Truncated,
    /// Not a RIFF file or not WAVE type.
    MissingRIFF,
    /// No `fmt ` chunk, or a `fmt ` chunk that is too short.
    MissingFmt,
    /// The audio format is something we do not decode.
    UnsupportedFormat(u16),
    /// Bits per sample are not 8/16/24/32.
    UnsupportedBitsPerSample(u16),
    /// No `data` chunk, or it is empty.
    MissingData,
}

impl core::fmt::Display for WavError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated => f.write_str("WAV file truncated before header"),
            Self::MissingRIFF => f.write_str("not a RIFF/WAVE file"),
            Self::MissingFmt => f.write_str("WAV file has no fmt chunk"),
            Self::UnsupportedFormat(fmt) => {
                write!(f, "unsupported WAV audio format {fmt}")
            }
            Self::UnsupportedBitsPerSample(bits) => {
                write!(f, "unsupported bits per sample: {bits}")
            }
            Self::MissingData => f.write_str("WAV file has no data chunk"),
        }
    }
}

impl std::error::Error for WavError {}

/// Decodes a WAV file from raw bytes.
///
/// Converts to the engine's f32 format but does **not** resample or remix —
/// the caller decides what to do with the channel count and sample rate.
pub fn decode(bytes: &[u8]) -> Result<WavFile, WavError> {
    let mut pos = 0;

    // ── RIFF header ──────────────────────────────────────────────────────
    let riff = read_fourcc(bytes, &mut pos)?;
    let _file_size = read_u32_le(bytes, &mut pos)?;
    let wave = read_fourcc(bytes, &mut pos)?;

    if riff != *b"RIFF" || wave != *b"WAVE" {
        return Err(WavError::MissingRIFF);
    }

    // ── Chunks ───────────────────────────────────────────────────────────
    let mut format_tag: u16 = 0;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut bits_per_sample: u16 = 0;
    let mut data: &[u8] = &[];

    while pos + 8 <= bytes.len() {
        let id = read_fourcc(bytes, &mut pos)?;
        let size = read_u32_le(bytes, &mut pos)? as usize;
        // `checked_add`: `size` comes from the file, and on a 32-bit target
        // `pos + size` can wrap past the bounds check and then panic on the
        // slice below.
        let end = pos
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or(WavError::Truncated)?;
        let payload = &bytes[pos..end];
        // RIFF pads every odd-sized chunk with a byte that the size does not
        // count. Without skipping it the next chunk header reads one byte
        // early and the file looks like garbage.
        pos = end + (size & 1);

        match &id {
            b"fmt " => {
                if size < 16 {
                    return Err(WavError::MissingFmt);
                }
                format_tag = u16::from_le_bytes([payload[0], payload[1]]);
                channels = u16::from_le_bytes([payload[2], payload[3]]);
                sample_rate = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                bits_per_sample = u16::from_le_bytes([payload[14], payload[15]]);
            }
            b"data" => {
                data = payload;
                // Stop parsing: the spec says data is last; anything after it
                // is non-standard and we do not decode it.
                break;
            }
            _ => {
                // Skip unknown chunks (LIST, INFO, cue, etc.).
            }
        }
    }

    if channels == 0 || bits_per_sample == 0 {
        return Err(WavError::MissingFmt);
    }
    if format_tag != 1 && format_tag != 3 {
        return Err(WavError::UnsupportedFormat(format_tag));
    }
    if data.is_empty() {
        return Err(WavError::MissingData);
    }

    // ── Decode samples ───────────────────────────────────────────────────
    let bytes_per_sample = (bits_per_sample / 8) as usize;
    if bytes_per_sample == 0 || bytes_per_sample > 4 {
        return Err(WavError::UnsupportedBitsPerSample(bits_per_sample));
    }
    // A trailing partial sample is simply not decoded: `chunks_exact` stops at
    // the last whole one.
    let samples = match (format_tag, bits_per_sample) {
        (1, 8) => {
            // 8-bit PCM is unsigned: 0x80 = zero.
            data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect()
        }
        (1, 16) => decode_s16(data),
        (1, 24) => decode_s24(data),
        (1, 32) => decode_s32(data),
        (3, 32) => decode_f32(data),
        _ => return Err(WavError::UnsupportedBitsPerSample(bits_per_sample)),
    };

    Ok(WavFile {
        samples,
        sample_rate,
        channels,
    })
}

// ── Raw helpers ──────────────────────────────────────────────────────────────

fn read_fourcc(bytes: &[u8], pos: &mut usize) -> Result<[u8; 4], WavError> {
    if *pos + 4 > bytes.len() {
        return Err(WavError::Truncated);
    }
    let value = [
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
    ];
    *pos += 4;
    Ok(value)
}

fn read_u32_le(bytes: &[u8], pos: &mut usize) -> Result<u32, WavError> {
    if *pos + 4 > bytes.len() {
        return Err(WavError::Truncated);
    }
    let value = u32::from_le_bytes([
        bytes[*pos],
        bytes[*pos + 1],
        bytes[*pos + 2],
        bytes[*pos + 3],
    ]);
    *pos += 4;
    Ok(value)
}

fn decode_s16(data: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(data.len() / 2);
    for chunk in data.chunks_exact(2) {
        let raw = i16::from_le_bytes([chunk[0], chunk[1]]);
        out.push(f32::from(raw) / 32768.0);
    }
    out
}

fn decode_s24(data: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(data.len() / 3);
    for chunk in data.chunks_exact(3) {
        // Sign-extend the 24-bit value into an i32.
        let raw = i32::from_le_bytes([
            chunk[0],
            chunk[1],
            chunk[2],
            if chunk[2] & 0x80 != 0 { 0xFF } else { 0x00 },
        ]);
        out.push(raw as f32 / 8_388_608.0); // 2^23
    }
    out
}

fn decode_s32(data: &[u8]) -> Vec<f32> {
    let scale = 2_147_483_648.0_f32; // 2^31
    let mut out = Vec::with_capacity(data.len() / 4);
    for chunk in data.chunks_exact(4) {
        let raw = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        out.push(raw as f32 / scale);
    }
    out
}

/// Float WAV is the one format that can carry NaN and infinities.
///
/// They are replaced with silence here, at the decoder boundary: `f32::clamp`
/// returns NaN for NaN so the mixer's clip would not catch them, and a NaN that
/// reaches a playhead calculation makes the voice immortal.
fn decode_f32(data: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(data.len() / 4);
    for chunk in data.chunks_exact(4) {
        let raw = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        out.push(if raw.is_finite() { raw } else { 0.0 });
    }
    out
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest valid WAV file: 44 bytes (canonical header).
    fn minimal_wav(data: &[u8], sample_rate: u32, channels: u16, bits: u16) -> Vec<u8> {
        let data_size = data.len() as u32;
        let file_size = 36 + data_size;
        let byte_rate = sample_rate * u32::from(channels) * u32::from(bits) / 8;
        let block_align = channels * bits / 8;

        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&file_size.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_size.to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn decode_mono_16bit_48k() {
        // One sample: i16::MAX = 32767 → nearly 1.0f32
        let raw: [u8; 2] = [0xFF, 0x7F]; // 32767 in little-endian
        let bytes = minimal_wav(&raw, 48000, 1, 16);
        let wav = decode(&bytes).unwrap();
        assert_eq!(wav.sample_rate, 48000);
        assert_eq!(wav.channels, 1);
        assert_eq!(wav.samples.len(), 1);
        assert!((wav.samples[0] - 32767.0 / 32768.0).abs() < 1e-6);
    }

    #[test]
    fn decode_stereo_16bit_44100() {
        // Two frames: (0.5, -0.5), (1.0, -1.0)
        let samples: [i16; 4] = [16384, -16384, 32767, -32768];
        let raw: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let bytes = minimal_wav(&raw, 44100, 2, 16);
        let wav = decode(&bytes).unwrap();
        assert_eq!(wav.sample_rate, 44100);
        assert_eq!(wav.channels, 2);
        assert_eq!(wav.samples.len(), 4);
        assert!((wav.samples[0] - 0.5).abs() < 0.001);
        assert!((wav.samples[1] + 0.5).abs() < 0.001);
        assert!((wav.samples[2] - 0.99997).abs() < 0.001);
        assert!((wav.samples[3] + 1.0).abs() < 0.001);
    }

    #[test]
    fn decode_8bit_unsigned() {
        // 8-bit PCM: 0x00 = -1.0, 0x80 = 0.0, 0xFF = ~1.0
        let raw = [0x80u8]; // 128 = zero
        let bytes = minimal_wav(&raw, 8000, 1, 8);
        let wav = decode(&bytes).unwrap();
        assert!((wav.samples[0] - 0.0).abs() < 0.01);
    }

    #[test]
    fn decode_24bit() {
        // 0x000000 = 0.0, 0x000080 = -0.5 (?), let's do a simple one
        let raw = [0x00u8, 0x00, 0x80]; // -8388608 → -1.0
        let bytes = minimal_wav(&raw, 48000, 1, 24);
        let wav = decode(&bytes).unwrap();
        assert!((wav.samples[0] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn decode_32bit_signed() {
        let val: i32 = 1_073_741_824; // 0.5 * 2^31
        let raw: Vec<u8> = val.to_le_bytes().to_vec();
        let bytes = minimal_wav(&raw, 48000, 1, 32);
        let wav = decode(&bytes).unwrap();
        assert!((wav.samples[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn decode_float32() {
        let val: f32 = -0.75;
        let raw: Vec<u8> = val.to_le_bytes().to_vec();
        let mut bytes = minimal_wav(&raw, 48000, 1, 32);
        // Patch format tag from 1 (PCM) to 3 (float).
        bytes[20] = 3;
        bytes[21] = 0;
        let wav = decode(&bytes).unwrap();
        assert!((wav.samples[0] + 0.75).abs() < 1e-6);
    }

    #[test]
    fn float32_non_finite_becomes_silence() {
        // NaN survives `f32::clamp`, and a NaN sample fed to a playhead makes
        // the voice immortal — so it is stopped at the decoder.
        let raw: Vec<u8> = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.75]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let mut bytes = minimal_wav(&raw, 48000, 1, 32);
        bytes[20] = 3; // IEEE float
        bytes[21] = 0;
        let wav = decode(&bytes).unwrap();
        assert_eq!(wav.samples, vec![0.0, 0.0, 0.0, -0.75]);
    }

    #[test]
    fn decodes_with_odd_sized_chunk_before_data() {
        // RIFF pads an odd-sized chunk with a byte the size field does not
        // count. Advancing by the size alone reads the next chunk header one
        // byte early, and the whole file fails to parse.
        let raw = [0xFFu8, 0x7F]; // one 16-bit mono sample
        let mut wav = minimal_wav(&raw, 44100, 1, 16);
        let data_chunk = wav.split_off(36);
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&5u32.to_le_bytes()); // odd payload
        wav.extend_from_slice(b"INFOx");
        wav.push(0); // the pad byte
        wav.extend_from_slice(&data_chunk);
        let new_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&new_size.to_le_bytes());

        let wav = decode(&wav).unwrap();
        assert_eq!(wav.channels, 1);
        assert_eq!(wav.samples.len(), 1);
        assert!((wav.samples[0] - 32767.0 / 32768.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_non_pcm_float_format() {
        let bytes = minimal_wav(&[0u8; 4], 44100, 1, 16);
        let mut alt = bytes.clone();
        alt[20] = 2; // ADPCM
        alt[21] = 0;
        assert_eq!(decode(&alt), Err(WavError::UnsupportedFormat(2)));
    }

    #[test]
    fn rejects_truncated() {
        assert_eq!(decode(b"RIFF"), Err(WavError::Truncated));
        // 20 bytes: RIFF header + "fmt " + size field, but no chunk body.
        assert_eq!(
            decode(&minimal_wav(&[0u8; 2], 44100, 1, 16)[..20]),
            Err(WavError::Truncated)
        );
    }

    #[test]
    fn rejects_non_riff() {
        let mut bytes = minimal_wav(&[0u8; 2], 44100, 1, 16);
        bytes[0] = b'X';
        assert_eq!(decode(&bytes), Err(WavError::MissingRIFF));
    }

    #[test]
    fn decodes_with_trailing_junk() {
        // RIFF spec says size gives the real end; trailing bytes are safe.
        let mut bytes = minimal_wav(&[0xFF, 0x7F], 48000, 1, 16);
        bytes.extend_from_slice(b"junk data after the chunks");
        let wav = decode(&bytes).unwrap();
        assert_eq!(wav.samples.len(), 1);
    }

    #[test]
    fn decodes_with_unknown_chunks_before_data() {
        // Build a WAV with a JUNK chunk (padding) between fmt and data.
        let raw = [0u8; 2]; // one 16-bit mono sample of silence
        let mut wav = minimal_wav(&raw, 44100, 1, 16);
        // Split after fmt chunk (first 36 bytes), insert JUNK, then data.
        let data_chunk = wav.split_off(36);
        wav.extend_from_slice(b"JUNK");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(b"\0\0\0\0"); // 4 bytes of junk
        wav.extend_from_slice(&data_chunk);
        // Fix RIFF size.
        let new_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&new_size.to_le_bytes());

        let wav = decode(&wav).unwrap();
        assert_eq!(wav.samples.len(), 1);
        assert_eq!(wav.channels, 1);
    }
}
