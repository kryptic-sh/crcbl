//! CRC-32 — the IEEE 802.3 polynomial in its reflected form, as zlib, PNG and
//! gzip compute it — the workspace's one copy.
//!
//! Corruption detection, never security: anyone can compute a matching CRC
//! for any bytes. [`synced`](crate::synced) uses it to tell a damaged file
//! from an intact one, and the PNG fixtures in `crcbl-sprite`'s and
//! `crcbl-golden`'s tests build chunk CRCs with it.

/// The reflected polynomial `0x04C11DB7`.
const POLYNOMIAL: u32 = 0xEDB8_8320;

/// The CRC of every byte value, built at compile time.
const TABLE: [u32; 256] = {
    let mut table = [0; 256];
    let mut byte = 0;
    while byte < 256 {
        let mut crc = byte as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLYNOMIAL
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[byte] = crc;
        byte += 1;
    }
    table
};

/// The CRC-32 of `bytes`.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    crc32_continue(0, bytes)
}

/// The CRC-32 of the bytes `previous` was computed over followed by `bytes`:
/// `crc32_continue(crc32(a), b) == crc32(a ++ b)`, as zlib's `crc32(crc,
/// buf, len)` resumes. PNG's chunk CRC, over the type and then the data, is
/// one such run.
#[must_use]
pub fn crc32_continue(previous: u32, bytes: &[u8]) -> u32 {
    let mut crc = !previous;
    for &byte in bytes {
        crc = (crc >> 8) ^ TABLE[usize::from((crc as u8) ^ byte)];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_check_value() {
        // The check value every CRC-32/ISO-HDLC catalogue lists.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn a_run_resumes_where_it_stopped() {
        let whole = crc32(b"IHDR and its data");
        assert_eq!(crc32_continue(crc32(b"IHDR"), b" and its data"), whole);
        assert_eq!(crc32_continue(0, b"IHDR and its data"), whole);
    }
}
