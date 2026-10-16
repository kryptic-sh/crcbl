//! VP8's boolean entropy decoder, RFC 6386 section 7.
//!
//! Every field of a VP8 frame past its first ten bytes is a run of bools,
//! each coded at a probability the decoder already knows; this is the
//! decoder the RFC gives in section 7.3, with its bit-at-a-time
//! renormalisation done as one shift.
//!
//! # The end of a partition
//!
//! The RFC's reference decoder shifts in zeros past the end of a partition.
//! So does this one, but it also notes when a bool was read with the
//! decoder's eight-bit window reaching past the last byte: that is where
//! libwebp stops with "premature end of file", and the frame decoder turns
//! it into [`ImageError::Malformed`](crate::ImageError::Malformed). An
//! encoder's flush always writes enough bytes that a whole frame never gets
//! there. This decoder holds one byte more than the window, so it reads a
//! byte past the end before the window needs it, and the check is on the
//! window, not on that read.

/// The bits of one partition, decoded a bool at a time.
#[derive(Debug)]
pub(super) struct BoolDecoder<'a> {
    data: &'a [u8],
    /// The next byte to shift in; past the end, zeros shift in instead.
    at: usize,
    /// The window and the byte after it, less the left end of the interval.
    value: u32,
    /// The interval's width, between 128 and 255 between bools.
    range: u32,
    /// How many bits have been shifted out of `value` since its last byte.
    bit_count: u32,
    past_end: bool,
}

impl<'a> BoolDecoder<'a> {
    /// RFC 6386 section 7.3's `init_bool_decoder`: the first two bytes.
    pub(super) fn new(data: &'a [u8]) -> Self {
        let mut decoder = Self {
            data,
            at: 0,
            value: 0,
            range: 255,
            bit_count: 0,
            past_end: false,
        };
        decoder.value = (u32::from(decoder.next_byte()) << 8) | u32::from(decoder.next_byte());
        decoder
    }

    fn next_byte(&mut self) -> u8 {
        let byte = self.data.get(self.at).copied().unwrap_or(0);
        self.at = self.at.saturating_add(1);
        byte
    }

    /// Whether a bool has been read with the window past the last byte.
    pub(super) fn past_end(&self) -> bool {
        self.past_end
    }

    /// RFC 6386 section 7.3's `read_bool`: a bool whose probability of being
    /// `false` is `probability / 256`.
    pub(super) fn read(&mut self, probability: u8) -> bool {
        if self.at >= self.data.len() {
            // The window covers bits `8 * (at - 2) + bit_count` onwards, eight
            // of them; past the data, it is incomplete.
            // Counted in u64: a large partition's bit count overflows a 32-bit
            // usize.
            let window_end = 8 * self.at.saturating_sub(2) as u64 + u64::from(self.bit_count) + 8;
            self.past_end |= window_end > 8 * self.data.len() as u64;
        }
        let split = 1 + (((self.range - 1) * u32::from(probability)) >> 8);
        let big_split = split << 8;
        let bit = if self.value >= big_split {
            self.range -= split;
            self.value -= big_split;
            true
        } else {
            self.range = split;
            false
        };
        if self.range < 128 {
            // The RFC doubles `range` and `value` until `range` is at least
            // 128, shifting a byte in each eighth time; at most seven shifts,
            // so at most one byte, which lands as far up as the shifts after
            // it would have moved it.
            let shift = self.range.leading_zeros() - 24;
            self.range <<= shift;
            self.value <<= shift;
            self.bit_count += shift;
            if self.bit_count >= 8 {
                self.bit_count -= 8;
                self.value |= u32::from(self.next_byte()) << self.bit_count;
            }
        }
        bit
    }

    /// A bool at even odds: the RFC's `F`.
    pub(super) fn flag(&mut self) -> bool {
        self.read(128)
    }

    /// The RFC's `L(n)`: `bits` flags, most significant first.
    pub(super) fn literal(&mut self, bits: u32) -> u32 {
        (0..bits).fold(0, |value, _| (value << 1) | u32::from(self.flag()))
    }

    /// A magnitude of `bits` flags and then a sign flag, set for negative: how
    /// the frame header codes its signed fields.
    pub(super) fn signed(&mut self, bits: u32) -> i32 {
        let magnitude = self.literal(bits) as i32;
        if self.flag() { -magnitude } else { magnitude }
    }

    /// A flag and, if it is set, [`Self::signed`]; zero otherwise.
    pub(super) fn maybe_signed(&mut self, bits: u32) -> i32 {
        if self.flag() { self.signed(bits) } else { 0 }
    }

    /// RFC 6386 section 8.1's `treed_read`, starting at node `start`.
    pub(super) fn read_tree_from(
        &mut self,
        tree: &[i8],
        probabilities: &[u8],
        start: usize,
    ) -> usize {
        let mut node = start;
        loop {
            let next = tree[node + usize::from(self.read(probabilities[node >> 1]))];
            if next <= 0 {
                return usize::from(next.unsigned_abs());
            }
            node = next as usize;
        }
    }

    /// RFC 6386 section 8.1's `treed_read`.
    pub(super) fn read_tree(&mut self, tree: &[i8], probabilities: &[u8]) -> usize {
        self.read_tree_from(tree, probabilities, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 6386 section 7.3's `bool_encoder`, transcribed, to make the bytes
    /// the decoder is checked against.
    struct Encoder {
        output: Vec<u8>,
        range: u32,
        bottom: u32,
        bit_count: i32,
    }

    impl Encoder {
        fn new() -> Self {
            Self {
                output: Vec::new(),
                range: 255,
                bottom: 0,
                bit_count: 24,
            }
        }

        fn add_one_to_output(&mut self) {
            let mut at = self.output.len();
            while at > 0 && self.output[at - 1] == 255 {
                self.output[at - 1] = 0;
                at -= 1;
            }
            self.output[at - 1] += 1;
        }

        fn write(&mut self, probability: u8, bit: bool) {
            let split = 1 + (((self.range - 1) * u32::from(probability)) >> 8);
            if bit {
                self.bottom = self.bottom.wrapping_add(split);
                self.range -= split;
            } else {
                self.range = split;
            }
            while self.range < 128 {
                self.range <<= 1;
                if self.bottom & (1 << 31) != 0 {
                    self.add_one_to_output();
                }
                self.bottom <<= 1;
                self.bit_count -= 1;
                if self.bit_count == 0 {
                    self.output.push((self.bottom >> 24) as u8);
                    self.bottom &= (1 << 24) - 1;
                    self.bit_count = 8;
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            let mut c = self.bit_count;
            let mut v = self.bottom;
            if v & (1 << (32 - c)) != 0 {
                self.add_one_to_output();
            }
            v <<= c & 7;
            c >>= 3;
            while c > 0 {
                v <<= 8;
                c -= 1;
            }
            for _ in 0..4 {
                self.output.push((v >> 24) as u8);
                v <<= 8;
            }
            self.output
        }
    }

    /// A pseudo-random walk over probabilities and bits, so every split and
    /// every renormalisation length is exercised.
    fn sequence() -> Vec<(u8, bool)> {
        let mut state = 0x2545_f491_u32;
        (0..4000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let probability = (state % 255) as u8 + 1;
                // Bits mostly as likely as their probability says, so the
                // range moves the way real data moves it.
                let bit = (state >> 8) % 256 >= u32::from(probability);
                (probability, bit)
            })
            .collect()
    }

    #[test]
    fn bools_read_back_as_the_rfcs_encoder_wrote_them() {
        let sequence = sequence();
        let mut encoder = Encoder::new();
        for &(probability, bit) in &sequence {
            encoder.write(probability, bit);
        }
        let bytes = encoder.finish();
        let mut decoder = BoolDecoder::new(&bytes);
        for (i, &(probability, bit)) in sequence.iter().enumerate() {
            assert_eq!(decoder.read(probability), bit, "bool {i}");
        }
        assert!(!decoder.past_end());
    }

    #[test]
    fn literals_trees_and_signed_values_read_as_the_rfc_codes_them() {
        let mut encoder = Encoder::new();
        // L(7) = 0b1011001, then a signed L(4) of -5, then the tree path
        // "1, 1, 0" of a four-leaf tree.
        for bit in [1, 0, 1, 1, 0, 0, 1] {
            encoder.write(128, bit == 1);
        }
        for bit in [0, 1, 0, 1, 1] {
            encoder.write(128, bit == 1);
        }
        let tree: [i8; 6] = [-0, 2, -1, 4, -2, -3];
        let probabilities = [30, 200, 90];
        encoder.write(30, true);
        encoder.write(200, true);
        encoder.write(90, false);
        let bytes = encoder.finish();
        let mut decoder = BoolDecoder::new(&bytes);
        assert_eq!(decoder.literal(7), 0b101_1001);
        assert_eq!(decoder.signed(4), -5);
        assert_eq!(decoder.read_tree(&tree, &probabilities), 2);
    }

    /// The window reaching past the data is noted; zeros shift in all the
    /// same, as in the RFC's reference decoder.
    #[test]
    fn reading_past_the_last_byte_is_noted() {
        let mut decoder = BoolDecoder::new(&[0x00]);
        assert!(!decoder.read(128));
        assert!(!decoder.past_end(), "the first window is the one byte");
        for _ in 0..8 {
            decoder.read(128);
        }
        assert!(decoder.past_end());
        let mut empty = BoolDecoder::new(&[]);
        empty.read(1);
        assert!(empty.past_end(), "an empty partition has no first window");
    }
}
