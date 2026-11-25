//! stb_image's Huffman tables and block decoders, transcribed: how it builds
//! a table (`stbi__build_huffman`, `stbi__build_fast_ac`), decodes a symbol
//! (`stbi__jpeg_huff_decode`) and decodes a block, sequential
//! (`stbi__jpeg_decode_block`) and progressive
//! (`stbi__jpeg_decode_block_prog_dc`, `stbi__jpeg_decode_block_prog_ac`).
//!
//! Each failure here is one of stb_image's, and it refuses the file for it.

use super::stream::{FAST_BITS, Stream};

const FAST_SIZE: usize = 1 << FAST_BITS;

/// Where each zigzag position's coefficient sits, and past the end the last
/// position again: `stbi__jpeg_dezigzag`, whose padding lets a damaged run
/// write the last coefficient instead of past the block.
const DEZIGZAG: [usize; 64 + 15] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63,
];

/// A natural-order index from a zigzag one: the dezigzag table, which
/// stb_image also uses to read a quantisation table.
pub(super) fn dezigzag(k: usize) -> usize {
    DEZIGZAG[k]
}

/// stb_image's failure, refused as malformed: its message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Corrupt(pub(super) &'static str);

/// `stbi__huffman`, and the `fast_ac` table stb_image keeps beside an AC
/// one. A table no `DHT` defined is all zeros, as `memset` leaves it: it
/// decodes every symbol as 0 from no bits.
#[derive(Clone, Debug)]
pub(super) struct Huffman {
    fast: [u8; FAST_SIZE],
    code: [u16; 256],
    values: [u8; 256],
    size: [u8; 257],
    maxcode: [u32; 18],
    delta: [i32; 17],
    fast_ac: [i16; FAST_SIZE],
}

impl Default for Huffman {
    fn default() -> Self {
        Self {
            fast: [0; FAST_SIZE],
            code: [0; 256],
            values: [0; 256],
            size: [0; 257],
            // `stbi__build_huffman`'s sentinel, which ends the search for a
            // long code. A table no marker defined decodes from its fast
            // table alone, but the search stays bounded anyway.
            maxcode: {
                let mut maxcode = [0; 18];
                maxcode[17] = u32::MAX;
                maxcode
            },
            delta: [0; 17],
            fast_ac: [0; FAST_SIZE],
        }
    }
}

impl Huffman {
    /// `stbi__build_huffman`, from the count of codes of each length. The
    /// values come after, into [`values_mut`](Self::values_mut).
    pub(super) fn build(&mut self, count: &[u8; 16]) -> Result<(), Corrupt> {
        let mut k = 0usize;
        for (i, &n) in count.iter().enumerate() {
            for _ in 0..n {
                if k >= 256 {
                    return Err(Corrupt("bad size list"));
                }
                self.size[k] = i as u8 + 1;
                k += 1;
            }
        }
        self.size[k] = 0;
        let mut code = 0u32;
        k = 0;
        for j in 1..=16 {
            self.delta[j] = k as i32 - code as i32;
            if usize::from(self.size[k]) == j {
                while usize::from(self.size[k]) == j {
                    self.code[k] = code as u16;
                    code += 1;
                    k += 1;
                }
                if code > 1 << j {
                    return Err(Corrupt("bad code lengths"));
                }
            }
            self.maxcode[j] = code << (16 - j);
            code <<= 1;
        }
        self.maxcode[17] = u32::MAX;
        self.fast = [u8::MAX; FAST_SIZE];
        for i in 0..k {
            let s = u32::from(self.size[i]);
            if s <= FAST_BITS {
                let c = usize::from(self.code[i]) << (FAST_BITS - s);
                let m = 1 << (FAST_BITS - s);
                self.fast[c..c + m].fill(i as u8);
            }
        }
        Ok(())
    }

    pub(super) fn values_mut(&mut self) -> &mut [u8; 256] {
        &mut self.values
    }

    /// `stbi__build_fast_ac`: for each leading nine bits that hold a whole
    /// short code and its magnitude bits, the run, the value and the bits
    /// both take.
    pub(super) fn build_fast_ac(&mut self) {
        for i in 0..FAST_SIZE {
            let fast = self.fast[i];
            self.fast_ac[i] = 0;
            if fast == u8::MAX {
                continue;
            }
            let rs = i32::from(self.values[usize::from(fast)]);
            let run = (rs >> 4) & 15;
            let magbits = (rs & 15) as u32;
            let len = u32::from(self.size[usize::from(fast)]);
            if magbits != 0 && len + magbits <= FAST_BITS {
                let mut k = (((i as i32) << len) & (FAST_SIZE as i32 - 1)) >> (FAST_BITS - magbits);
                let m = 1 << (magbits - 1);
                if k < m {
                    k += 1 - (1 << magbits);
                }
                if (-128..=127).contains(&k) {
                    self.fast_ac[i] = (k * 256 + run * 16 + (len + magbits) as i32) as i16;
                }
            }
        }
    }

    /// `stbi__jpeg_huff_decode`: the next symbol, or `None` for a code the
    /// table lacks or one the buffer holds too few bits of.
    fn decode(&self, stream: &mut Stream<'_>) -> Option<u8> {
        if stream.code_bits() < 16 {
            stream.grow();
        }
        let k = self.fast[stream.peek_fast()];
        if k < u8::MAX {
            let s = self.size[usize::from(k)];
            if i32::from(s) > stream.code_bits() {
                return None;
            }
            stream.consume(u32::from(s));
            return Some(self.values[usize::from(k)]);
        }
        let temp = stream.peek16();
        let mut k = FAST_BITS as usize + 1;
        while temp >= self.maxcode[k] {
            k += 1;
        }
        if k == 17 {
            stream.drop_sixteen();
            return None;
        }
        if k as i32 > stream.code_bits() {
            return None;
        }
        let c = stream.peek(k as u32) as i32 + self.delta[k];
        let c = usize::try_from(c).ok().filter(|&c| c < 256)?;
        stream.consume(k as u32);
        Some(self.values[c])
    }
}

/// `stbi__addints_valid` and the add: the DC prediction plus a difference,
/// refused where an `int` would overflow.
fn add_dc(prediction: &mut i32, diff: i32) -> Result<i32, Corrupt> {
    *prediction = prediction.checked_add(diff).ok_or(Corrupt("bad delta"))?;
    Ok(*prediction)
}

/// `stbi__mul2shorts_valid` and the product: refused unless it fits a
/// `short`.
fn dc_times(dc: i32, factor: i32) -> Result<i16, Corrupt> {
    i16::try_from(i64::from(dc) * i64::from(factor)).map_err(|_| Corrupt("can't merge dc and ac"))
}

/// `stbi__jpeg_decode_block`: a sequential block, dequantised by `dequant`
/// as it is read and kept to 16 bits as stb_image's `short` keeps it.
pub(super) fn decode_block(
    stream: &mut Stream<'_>,
    data: &mut [i16; 64],
    (hdc, hac): (&Huffman, &Huffman),
    dc_pred: &mut i32,
    dequant: &[u16; 64],
) -> Result<(), Corrupt> {
    if stream.code_bits() < 16 {
        stream.grow();
    }
    let t = hdc
        .decode(stream)
        .filter(|&t| t <= 15)
        .ok_or(Corrupt("bad huffman code"))?;
    *data = [0; 64];
    let diff = if t == 0 {
        0
    } else {
        stream.extend_receive(u32::from(t))
    };
    let dc = add_dc(dc_pred, diff)?;
    data[0] = dc_times(dc, i32::from(dequant[0]))?;
    let mut k = 1;
    loop {
        if stream.code_bits() < 16 {
            stream.grow();
        }
        let r = i32::from(hac.fast_ac[stream.peek_fast()]);
        if r != 0 {
            k += ((r >> 4) & 15) as usize;
            let s = r & 15;
            if s > stream.code_bits() {
                return Err(Corrupt("bad huffman code"));
            }
            stream.consume(s as u32);
            let zig = DEZIGZAG[k];
            k += 1;
            data[zig] = ((r >> 8) * i32::from(dequant[zig])) as i16;
        } else {
            let rs = hac.decode(stream).ok_or(Corrupt("bad huffman code"))?;
            let (r, s) = (usize::from(rs >> 4), u32::from(rs & 15));
            if s == 0 {
                if rs != 0xf0 {
                    break;
                }
                k += 16;
            } else {
                k += r;
                let zig = DEZIGZAG[k];
                k += 1;
                data[zig] = (stream.extend_receive(s) * i32::from(dequant[zig])) as i16;
            }
        }
        if k >= 64 {
            break;
        }
    }
    Ok(())
}

/// What a progressive scan's header set: its band and its successive
/// approximation.
#[derive(Clone, Copy, Debug)]
pub(super) struct Band {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) high: u32,
    pub(super) low: u32,
}

/// `stbi__jpeg_decode_block_prog_dc`: a DC first pass clears the block and
/// sets the DC; a refinement adds its bit.
pub(super) fn decode_block_prog_dc(
    stream: &mut Stream<'_>,
    data: &mut [i16; 64],
    hdc: &Huffman,
    dc_pred: &mut i32,
    band: Band,
) -> Result<(), Corrupt> {
    if band.end != 0 {
        return Err(Corrupt("can't merge dc and ac"));
    }
    if stream.code_bits() < 16 {
        stream.grow();
    }
    if band.high == 0 {
        *data = [0; 64];
        let t = hdc
            .decode(stream)
            .filter(|&t| t <= 15)
            .ok_or(Corrupt("can't merge dc and ac"))?;
        let diff = if t == 0 {
            0
        } else {
            stream.extend_receive(u32::from(t))
        };
        let dc = add_dc(dc_pred, diff)?;
        data[0] = dc_times(dc, 1 << band.low)?;
    } else if stream.get_bit() {
        data[0] = data[0].wrapping_add(1 << band.low);
    }
    Ok(())
}

/// `stbi__jpeg_decode_block_prog_ac`: a band's first pass or a refinement,
/// with the end-of-band run carried in `eob_run`.
pub(super) fn decode_block_prog_ac(
    stream: &mut Stream<'_>,
    data: &mut [i16; 64],
    hac: &Huffman,
    band: Band,
    eob_run: &mut i32,
) -> Result<(), Corrupt> {
    if band.start == 0 {
        return Err(Corrupt("can't merge dc and ac"));
    }
    if band.high == 0 {
        if *eob_run != 0 {
            *eob_run -= 1;
            return Ok(());
        }
        let mut k = band.start;
        loop {
            if stream.code_bits() < 16 {
                stream.grow();
            }
            let r = i32::from(hac.fast_ac[stream.peek_fast()]);
            if r != 0 {
                k += ((r >> 4) & 15) as usize;
                let s = r & 15;
                if s > stream.code_bits() {
                    return Err(Corrupt("bad huffman code"));
                }
                stream.consume(s as u32);
                let zig = DEZIGZAG[k];
                k += 1;
                data[zig] = ((r >> 8) * (1 << band.low)) as i16;
            } else {
                let rs = hac.decode(stream).ok_or(Corrupt("bad huffman code"))?;
                let (r, s) = (u32::from(rs >> 4), u32::from(rs & 15));
                if s == 0 {
                    if r < 15 {
                        *eob_run = 1 << r;
                        if r != 0 {
                            *eob_run += stream.get_bits(r) as i32;
                        }
                        *eob_run -= 1;
                        break;
                    }
                    k += 16;
                } else {
                    k += r as usize;
                    let zig = DEZIGZAG[k];
                    k += 1;
                    data[zig] = (stream.extend_receive(s) * (1 << band.low)) as i16;
                }
            }
            if k > band.end {
                break;
            }
        }
        return Ok(());
    }

    let bit = 1i16 << band.low;
    // A nonzero coefficient takes a correction bit wherever the band passes
    // it, unless it already has that bit.
    let refine = |stream: &mut Stream<'_>, p: &mut i16| {
        if stream.get_bit() && *p & bit == 0 {
            *p = if *p > 0 {
                p.wrapping_add(bit)
            } else {
                p.wrapping_sub(bit)
            };
        }
    };
    if *eob_run != 0 {
        *eob_run -= 1;
        for &zig in &DEZIGZAG[band.start..=band.end] {
            if data[zig] != 0 {
                refine(stream, &mut data[zig]);
            }
        }
        return Ok(());
    }
    let mut k = band.start;
    loop {
        let rs = hac.decode(stream).ok_or(Corrupt("bad huffman code"))?;
        let (mut r, s) = (i32::from(rs >> 4), rs & 15);
        let mut value = 0i16;
        if s == 0 {
            if r < 15 {
                *eob_run = (1 << r) - 1;
                if r != 0 {
                    *eob_run += stream.get_bits(r as u32) as i32;
                }
                // Past every coefficient: the rest of the band is refined.
                r = 64;
            }
        } else {
            if s != 1 {
                return Err(Corrupt("bad huffman code"));
            }
            value = if stream.get_bit() { bit } else { -bit };
        }
        while k <= band.end {
            let zig = DEZIGZAG[k];
            k += 1;
            if data[zig] != 0 {
                refine(stream, &mut data[zig]);
            } else {
                if r == 0 {
                    data[zig] = value;
                    break;
                }
                r -= 1;
            }
        }
        if k > band.end {
            break;
        }
    }
    Ok(())
}
