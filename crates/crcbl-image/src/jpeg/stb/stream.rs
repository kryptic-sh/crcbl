//! stb_image's reading of the bytes: its memory context (`stbi__get8` and
//! the rest), its marker reads, and the entropy decoder's bit buffer
//! (`stbi__grow_buffer_unsafe` and the readers over it).
//!
//! The bit buffer is transcribed exactly, because what stb_image reads at a
//! marker depends on when it refills: the refill that meets a marker stops
//! there, so a read that needs more bits than are left fails or reads zero
//! (as each reader does), while every refill after it pads with zero bits.

/// stb_image's `STBI__MARKER_none`: no marker, a value no marker has.
pub(super) const NO_MARKER: u8 = 0xff;

/// `(1 << n) - 1`: stb_image's `stbi__bmask`.
const fn bmask(n: u32) -> u32 {
    (1 << n) - 1
}

/// A file being read as stb_image reads it, with the entropy decoder's
/// state: `stbi__context` over a memory buffer, and the bit-buffer fields of
/// `stbi__jpeg`.
#[derive(Debug)]
pub(super) struct Stream<'a> {
    data: &'a [u8],
    /// May pass the end, as `stbi__skip` leaves a memory context's pointer.
    pos: usize,
    code_buffer: u32,
    /// Bits in `code_buffer`; stb_image keeps it in an `int` and lets an
    /// error path take it below zero.
    code_bits: i32,
    /// A marker the entropy decoder met, or [`NO_MARKER`].
    pub(super) marker: u8,
    /// Whether a marker has been met, so refills read zeros.
    nomore: bool,
    /// The bits of `code_buffer` a refill made up, as zeros past a marker
    /// or the end of the file: always its last ones, since nothing real
    /// follows them until a reset.
    padding_in_buffer: i32,
    /// The reads before the one in progress that were made up, for the work
    /// bound. A read is a Huffman code with the magnitude or run bits read
    /// after it, or a lone bit; it is made up if it took a made-up bit out
    /// of the buffer, or gave a value the buffer could not hold bits for.
    /// A refill reads ahead of what the decoder needs, so only bits taken
    /// count.
    padded_reads: u64,
    /// Whether the read in progress is made up.
    read_made_up: bool,
    /// Whether the count is owed anything: made-up bits in the buffer, or a
    /// made-up read not yet counted. Never true while real data is read, so
    /// a take tests this one field and is otherwise just stb_image's.
    counting: bool,
}

impl<'a> Stream<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            code_buffer: 0,
            code_bits: 0,
            marker: NO_MARKER,
            nomore: false,
            padding_in_buffer: 0,
            padded_reads: 0,
            read_made_up: false,
            counting: false,
        }
    }

    /// `stbi__get8`: the next byte, or 0 past the end.
    pub(super) fn get8(&mut self) -> u8 {
        match self.data.get(self.pos) {
            Some(&byte) => {
                self.pos += 1;
                byte
            }
            None => 0,
        }
    }

    /// `stbi__get16be`.
    pub(super) fn get16be(&mut self) -> i32 {
        let high = i32::from(self.get8());
        (high << 8) + i32::from(self.get8())
    }

    /// `stbi__at_eof`.
    pub(super) fn at_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// `stbi__skip`: forward `n` bytes, unchecked, or to the end for a
    /// negative `n`.
    pub(super) fn skip(&mut self, n: i32) {
        match usize::try_from(n) {
            Ok(n) => self.pos = self.pos.saturating_add(n),
            Err(_) => self.pos = self.data.len(),
        }
    }

    /// The file, and where the entropy-coded data of a scan starting here
    /// begins in it.
    pub(super) fn scan_data(&self) -> (&'a [u8], usize) {
        (self.data, self.pos)
    }

    /// The bytes from here to `len` further on, if the file holds them all.
    pub(super) fn ahead(&self, len: usize) -> Option<&'a [u8]> {
        self.data.get(self.pos..self.pos.checked_add(len)?)
    }

    /// `stbi__get_marker`: the marker the entropy decoder met, if any, else
    /// the next one here past fill bytes, or [`NO_MARKER`] if the next byte
    /// is not `0xff`.
    pub(super) fn get_marker(&mut self) -> u8 {
        if self.marker != NO_MARKER {
            return core::mem::replace(&mut self.marker, NO_MARKER);
        }
        let mut x = self.get8();
        if x != 0xff {
            return NO_MARKER;
        }
        while x == 0xff {
            x = self.get8();
        }
        x
    }

    /// `stbi__skip_jpeg_junk_at_end`: past bytes that are not a marker to
    /// the next that is, or [`NO_MARKER`] at the end.
    pub(super) fn skip_junk_at_end(&mut self) -> u8 {
        while !self.at_eof() {
            let mut x = self.get8();
            while x == 0xff {
                if self.at_eof() {
                    return NO_MARKER;
                }
                x = self.get8();
                if x != 0x00 && x != 0xff {
                    return x;
                }
            }
        }
        NO_MARKER
    }

    /// The bit buffer's part of `stbi__jpeg_reset`.
    pub(super) fn reset_bits(&mut self) {
        self.code_bits = 0;
        self.code_buffer = 0;
        self.padding_in_buffer = 0;
        self.recount();
        self.nomore = false;
        self.marker = NO_MARKER;
    }

    /// The made-up reads so far, over the whole file, the one in progress
    /// included.
    pub(super) fn made_up_reads(&self) -> u64 {
        self.padded_reads + u64::from(self.read_made_up)
    }

    /// Whether the reader has reached the end of the scan's data: a marker,
    /// or the end of the file.
    pub(super) fn past_data(&self) -> bool {
        self.nomore || self.at_eof()
    }

    /// Starts a read, counting the one before it if it was made up.
    #[inline(always)]
    fn begin_read(&mut self) {
        if self.counting {
            self.count_read();
        }
    }

    /// The part of [`begin_read`](Self::begin_read) for when the count is
    /// owed something.
    #[cold]
    fn count_read(&mut self) {
        if self.read_made_up {
            self.padded_reads += 1;
            self.read_made_up = false;
        }
        self.recount();
    }

    /// Marks the read in progress made up.
    fn made_up_read(&mut self) {
        self.read_made_up = true;
        self.counting = true;
    }

    /// Sets [`counting`](Self::counting) from what it stands for.
    fn recount(&mut self) {
        self.counting = self.padding_in_buffer != 0 || self.read_made_up;
    }

    pub(super) fn code_bits(&self) -> i32 {
        self.code_bits
    }

    /// The buffer's top nine bits: what stb_image's fast tables index by.
    pub(super) fn peek_fast(&self) -> usize {
        (self.code_buffer >> (32 - FAST_BITS)) as usize & ((1 << FAST_BITS) - 1)
    }

    /// The buffer's top sixteen bits, for a code longer than the fast
    /// tables hold.
    pub(super) fn peek16(&self) -> u32 {
        self.code_buffer >> 16
    }

    /// The buffer's top `k` bits, `k` from 1 to 16.
    pub(super) fn peek(&self, k: u32) -> u32 {
        (self.code_buffer >> (32 - k)) & bmask(k)
    }

    /// Drops `n` bits, `n` at most 16.
    pub(super) fn consume(&mut self, n: u32) {
        self.code_buffer <<= n;
        self.take(n as i32, true);
    }

    /// Takes `n` bits out of the buffer, as the start of a read if
    /// `starts_read`, marking the read made up if any of them was. Inlined,
    /// so `starts_read` folds away; the counting is kept out of line.
    #[inline(always)]
    fn take(&mut self, n: i32, starts_read: bool) {
        if self.counting {
            self.take_counted(n, starts_read);
        }
        self.code_bits -= n;
    }

    /// The part of [`take`](Self::take) for when the count is owed
    /// something: the read started, and how many of the `n` bits taken
    /// were made up, from the buffer's last bits.
    #[cold]
    fn take_counted(&mut self, n: i32, starts_read: bool) {
        if starts_read && self.read_made_up {
            self.padded_reads += 1;
            self.read_made_up = false;
        }
        let real = (self.code_bits - self.padding_in_buffer).max(0);
        let made_up = (n - real).clamp(0, self.padding_in_buffer);
        self.padding_in_buffer -= made_up;
        self.read_made_up |= made_up > 0;
        self.recount();
    }

    /// The error path of `stbi__jpeg_huff_decode` for a code past every
    /// length, which drops sixteen bits before failing.
    pub(super) fn drop_sixteen(&mut self) {
        self.take(16, true);
    }

    /// `stbi__grow_buffer_unsafe`: whole bytes into the buffer until it
    /// holds more than 24 bits, undoing byte stuffing. A marker stops it
    /// where it is, the first time; after that it reads zeros.
    pub(super) fn grow(&mut self) {
        loop {
            let made_up = self.nomore || self.at_eof();
            let b = if self.nomore { 0 } else { self.get8() };
            if b == 0xff {
                let mut c = self.get8();
                while c == 0xff {
                    c = self.get8();
                }
                if c != 0 {
                    self.marker = c;
                    self.nomore = true;
                    return;
                }
            }
            self.code_buffer |= u32::from(b) << (24 - self.code_bits);
            self.code_bits += 8;
            if made_up {
                self.padding_in_buffer += 8;
                self.counting = true;
            }
            if self.code_bits > 24 {
                return;
            }
        }
    }

    /// `stbi__extend_receive`: `n` bits as a signed value, `n` from 1 to
    /// 15, or 0 if the buffer cannot hold them.
    pub(super) fn extend_receive(&mut self, n: u32) -> i32 {
        if self.code_bits < n as i32 {
            self.grow();
        }
        if self.code_bits < n as i32 {
            self.made_up_read();
            return 0;
        }
        let negative_bias = if self.code_buffer >> 31 == 0 {
            -(bmask(n) as i32)
        } else {
            0
        };
        let k = self.code_buffer.rotate_left(n);
        self.code_buffer = k & !bmask(n);
        self.take(n as i32, false);
        (k & bmask(n)) as i32 + negative_bias
    }

    /// `stbi__jpeg_get_bits`: `n` bits unsigned, `n` from 1 to 14, or 0 if
    /// the buffer cannot hold them.
    pub(super) fn get_bits(&mut self, n: u32) -> u32 {
        if self.code_bits < n as i32 {
            self.grow();
        }
        if self.code_bits < n as i32 {
            self.made_up_read();
            return 0;
        }
        let k = self.code_buffer.rotate_left(n);
        self.code_buffer = k & !bmask(n);
        self.take(n as i32, false);
        k & bmask(n)
    }

    /// `stbi__jpeg_get_bit`, or false if the buffer cannot hold one: a read
    /// of its own. Inlined into the refinement passes, which call it for
    /// every correction bit.
    #[inline(always)]
    pub(super) fn get_bit(&mut self) -> bool {
        self.begin_read();
        if self.code_bits < 1 {
            self.grow();
        }
        if self.code_bits < 1 {
            self.made_up_read();
            return false;
        }
        let k = self.code_buffer;
        self.code_buffer <<= 1;
        self.take(1, false);
        k & 0x8000_0000 != 0
    }
}

/// stb_image's `FAST_BITS`: how many leading bits its fast tables index.
pub(super) const FAST_BITS: u32 = 9;

#[cfg(test)]
mod tests {
    use super::*;

    /// A field the buffer cannot hold, where the refill that meets the
    /// marker stops short, reads as 0 without bits: the read is made up,
    /// as the default decoder's zeros past a marker are.
    #[test]
    fn a_field_read_as_zero_without_bits_is_made_up() {
        let mut stream = Stream::new(&[0x12, 0x34, 0x56, 0x78, 0xff, 0xd9]);
        stream.grow();
        stream.consume(16);
        // A code, then a field longer than the twelve real bits left.
        stream.consume(4);
        assert_eq!(stream.made_up_reads(), 0);
        assert_eq!(stream.extend_receive(15), 0);
        assert_eq!(stream.made_up_reads(), 1);
    }
}
