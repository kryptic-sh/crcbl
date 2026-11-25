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
        self.nomore = false;
        self.marker = NO_MARKER;
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
        self.code_bits -= n as i32;
    }

    /// The error path of `stbi__jpeg_huff_decode` for a code past every
    /// length, which drops sixteen bits before failing.
    pub(super) fn drop_sixteen(&mut self) {
        self.code_bits -= 16;
    }

    /// `stbi__grow_buffer_unsafe`: whole bytes into the buffer until it
    /// holds more than 24 bits, undoing byte stuffing. A marker stops it
    /// where it is, the first time; after that it reads zeros.
    pub(super) fn grow(&mut self) {
        loop {
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
            return 0;
        }
        let negative_bias = if self.code_buffer >> 31 == 0 {
            -(bmask(n) as i32)
        } else {
            0
        };
        let k = self.code_buffer.rotate_left(n);
        self.code_buffer = k & !bmask(n);
        self.code_bits -= n as i32;
        (k & bmask(n)) as i32 + negative_bias
    }

    /// `stbi__jpeg_get_bits`: `n` bits unsigned, `n` from 1 to 14, or 0 if
    /// the buffer cannot hold them.
    pub(super) fn get_bits(&mut self, n: u32) -> u32 {
        if self.code_bits < n as i32 {
            self.grow();
        }
        if self.code_bits < n as i32 {
            return 0;
        }
        let k = self.code_buffer.rotate_left(n);
        self.code_buffer = k & !bmask(n);
        self.code_bits -= n as i32;
        k & bmask(n)
    }

    /// `stbi__jpeg_get_bit`, or false if the buffer cannot hold one.
    pub(super) fn get_bit(&mut self) -> bool {
        if self.code_bits < 1 {
            self.grow();
        }
        if self.code_bits < 1 {
            return false;
        }
        let k = self.code_buffer;
        self.code_buffer <<= 1;
        self.code_bits -= 1;
        k & 0x8000_0000 != 0
    }
}

/// stb_image's `FAST_BITS`: how many leading bits its fast tables index.
pub(super) const FAST_BITS: u32 = 9;
