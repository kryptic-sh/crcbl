//! stb_image's JPEG decoder, transcribed from stb_image 2.30's
//! `stb_image.h` as llama.cpp vendors it
//! (`llama.cpp/vendor/stb/stb_image.h`): how it walks the markers
//! (`stbi__decode_jpeg_header`, `stbi__decode_jpeg_image`,
//! `stbi__process_marker`), reads frame and scan headers, and runs a scan
//! (`stbi__parse_entropy_coded_data`). Its bit buffer and block decoders are
//! in [`stream`] and [`entropy`], its stages to pixels in [`pixels`]. The
//! [module docs](super) say what this guarantees and where it stops.
//!
//! Only the frame header is read through the crate's own code, for the
//! [`Limits`] check and the coefficient store, after stb_image's checks of
//! it.
//!
//! stb_image allocates its sample and coefficient buffers without clearing
//! them, and a block is only cleared when it is decoded: a sequential block
//! whole, a progressive one by its DC first pass. Where its output or its
//! course depends on a block not yet cleared, the file is refused here as
//! [`ImageError::Unsupported`], since stb_image's answer is whatever memory
//! held: a block it returns uncleared, an AC refinement reading one (the
//! coefficients it finds nonzero decide how many bits it reads), and a
//! progressive file it stops decoding early, which it never transforms.
//!
//! Apart from that, a file whose scans would do more work than their data
//! allows is refused as [`ImageError::TooMuchWork`], whether stb_image's
//! output for it is determined or not: a resource limit, counted as the
//! scans are walked (see the [work bound](super#work-bounded-by-the-input)).

mod entropy;
mod pixels;
mod stream;
#[cfg(test)]
pub(super) mod tests;

use entropy::{Band, Corrupt, Huffman, dezigzag};
use stream::{NO_MARKER, Stream};

use super::{Frame, Work, malformed, read_frame, unsupported};
use crate::{ImageError, Limits, Rgba8Image};

/// The colour markers `load_jpeg_image` converts by, as stb_image leaves
/// them at the end of the file: whether a JFIF `APP0` was read, and the
/// last Adobe `APP14` transform, -1 for none.
#[derive(Clone, Copy, Debug)]
struct Colour {
    jfif: bool,
    app14_color_transform: i32,
}

/// stb_image's failure, refused as malformed.
fn refused(Corrupt(why): Corrupt) -> ImageError {
    malformed(format!("stb_image refuses it: {why}"))
}

/// What stb_image reads from a block it never cleared.
fn uncleared(why: &str) -> ImageError {
    unsupported(format!(
        "{why} a block stb_image never cleared, so its result is whatever memory held"
    ))
}

/// Decodes a JPEG as stb_image does, its entropy decoding accounted in
/// `work`; see [`super::decode_as_stb_image`].
pub(super) fn decode(
    bytes: &[u8],
    limits: &Limits,
    work: &mut Work,
) -> Result<Rgba8Image, ImageError> {
    let mut jpeg = Jpeg::new(bytes);
    let mut frame = jpeg.decode_header(limits)?;
    jpeg.cleared = frame
        .components
        .iter()
        .map(|c| vec![false; c.blocks_wide * c.blocks_high])
        .collect();
    let ending = jpeg.decode_image(&mut frame, work)?;
    if frame.progressive && ending == Ending::Stopped {
        return Err(unsupported(
            "a progressive JPEG stb_image stops decoding before its end, so never transforms",
        ));
    }
    // The blocks it returns: each component's own, not the padding to whole
    // MCUs.
    for (component, cleared) in frame.components.iter().zip(&jpeg.cleared) {
        for by in 0..component.height.div_ceil(8) {
            for bx in 0..component.width.div_ceil(8) {
                if !cleared[by * component.blocks_wide + bx] {
                    return Err(uncleared("pixels from"));
                }
            }
        }
    }
    if frame.progressive {
        jpeg.dequantize(&mut frame);
    }
    pixels::finish(&frame, jpeg.colour, limits)
}

/// How `stbi__decode_jpeg_image` ended: at the end-of-image marker, or
/// early, at a marker `stbi__process_marker` failed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ending {
    EndOfImage,
    Stopped,
}

/// The parts of a frame component's state stb_image keeps beside its
/// samples.
#[derive(Clone, Copy, Debug, Default)]
struct ComponentState {
    hd: usize,
    ha: usize,
    dc_pred: i32,
}

/// A scan as its header set it: its components in order, and its band.
#[derive(Debug)]
struct Scan {
    order: Vec<usize>,
    band: Band,
}

/// `stbi__jpeg`: the tables and settings as the markers so far set them,
/// and the decoder's state.
#[derive(Debug)]
struct Jpeg<'a> {
    stream: Stream<'a>,
    huff_dc: [Huffman; 4],
    huff_ac: [Huffman; 4],
    dequant: [[u16; 64]; 4],
    colour: Colour,
    restart_interval: i32,
    components: [ComponentState; 4],
    /// For each frame component, which of its blocks stb_image has
    /// cleared: by decoding it, sequential, or by a DC first pass.
    cleared: Vec<Vec<bool>>,
    eob_run: i32,
    todo: i32,
}

impl<'a> Jpeg<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            stream: Stream::new(bytes),
            huff_dc: core::array::from_fn(|_| Huffman::default()),
            huff_ac: core::array::from_fn(|_| Huffman::default()),
            dequant: [[0; 64]; 4],
            colour: Colour {
                jfif: false,
                app14_color_transform: -1,
            },
            restart_interval: 0,
            components: [ComponentState::default(); 4],
            cleared: Vec::new(),
            eob_run: 0,
            todo: 0,
        }
    }

    /// `stbi__decode_jpeg_header`: the markers up to the frame, and the
    /// frame header. Bytes that are not a marker between two segments are
    /// skipped, as stb_image skips them.
    fn decode_header(&mut self, limits: &Limits) -> Result<Frame, ImageError> {
        if self.stream.get_marker() != 0xd8 {
            return Err(refused(Corrupt("no SOI")));
        }
        let mut m = self.stream.get_marker();
        while !matches!(m, 0xc0..=0xc2) {
            if !self.process_marker(m) {
                return Err(refused(Corrupt("a marker before the frame")));
            }
            m = self.stream.get_marker();
            while m == NO_MARKER {
                if self.stream.at_eof() {
                    return Err(refused(Corrupt("no SOF")));
                }
                m = self.stream.get_marker();
            }
        }
        self.process_frame_header(m == 0xc2, limits)
    }

    /// `stbi__process_frame_header`'s checks, then the frame as
    /// [`read_frame`] builds it, checked against `limits`.
    fn process_frame_header(
        &mut self,
        progressive: bool,
        limits: &Limits,
    ) -> Result<Frame, ImageError> {
        let s = &mut self.stream;
        let lf = s.get16be();
        if lf < 11 {
            return Err(refused(Corrupt("bad SOF len")));
        }
        let segment = s
            .ahead(lf as usize - 2)
            .ok_or_else(|| malformed("a frame header is cut short"))?;
        if s.get8() != 8 {
            return Err(unsupported("JPEG samples of other than 8 bits"));
        }
        let height = s.get16be();
        if height == 0 {
            return Err(unsupported("a JPEG whose height comes in a DNL marker"));
        }
        let width = s.get16be();
        let count = i32::from(s.get8());
        if !matches!(count, 1 | 3 | 4) {
            return Err(unsupported(format!("a JPEG of {count} components")));
        }
        if lf != 8 + 3 * count {
            return Err(refused(Corrupt("bad SOF len")));
        }
        for _ in 0..count {
            s.get8();
            let q = s.get8();
            if !(1..=4).contains(&(q >> 4)) || !(1..=4).contains(&(q & 15)) || s.get8() > 3 {
                return Err(refused(Corrupt("bad H, V or TQ")));
            }
        }
        // `stbi__mad3sizes_valid`: the samples must fit an `int`.
        if i64::from(width) * i64::from(height) * i64::from(count) > i64::from(i32::MAX) {
            return Err(unsupported("a JPEG larger than stb_image decodes"));
        }
        read_frame(segment, progressive, limits)
    }

    /// `stbi__process_marker`: a table, setting or `APPn` segment read, or
    /// false for one stb_image fails on, unknown markers among them.
    fn process_marker(&mut self, m: u8) -> bool {
        let s = &mut self.stream;
        match m {
            NO_MARKER => false,
            0xdd => {
                if s.get16be() != 4 {
                    return false;
                }
                self.restart_interval = s.get16be();
                true
            }
            0xdb => {
                let mut l = s.get16be() - 2;
                while l > 0 {
                    let q = s.get8();
                    let (p, t) = (q >> 4, usize::from(q & 15));
                    if p > 1 || t > 3 {
                        return false;
                    }
                    for i in 0..64 {
                        self.dequant[t][dezigzag(i)] = if p == 1 {
                            s.get16be() as u16
                        } else {
                            u16::from(s.get8())
                        };
                    }
                    l -= if p == 1 { 129 } else { 65 };
                }
                l == 0
            }
            0xc4 => {
                let mut l = s.get16be() - 2;
                while l > 0 {
                    let q = s.get8();
                    let (tc, th) = (q >> 4, usize::from(q & 15));
                    if tc > 1 || th > 3 {
                        return false;
                    }
                    let mut sizes = [0u8; 16];
                    for size in &mut sizes {
                        *size = s.get8();
                    }
                    let n: usize = sizes.iter().map(|&size| usize::from(size)).sum();
                    if n > 256 {
                        return false;
                    }
                    l -= 17;
                    let table = if tc == 0 {
                        &mut self.huff_dc[th]
                    } else {
                        &mut self.huff_ac[th]
                    };
                    if table.build(&sizes).is_err() {
                        return false;
                    }
                    for value in &mut table.values_mut()[..n] {
                        *value = s.get8();
                    }
                    if tc != 0 {
                        table.build_fast_ac();
                    }
                    l -= n as i32;
                }
                l == 0
            }
            0xe0..=0xef | 0xfe => {
                let mut l = s.get16be();
                if l < 2 {
                    return false;
                }
                l -= 2;
                // Every byte of a tag is read, matching or not.
                let mut tag = |tag: &[u8]| {
                    let mut ok = true;
                    for &t in tag {
                        ok &= s.get8() == t;
                    }
                    ok
                };
                if m == 0xe0 && l >= 5 {
                    let jfif = tag(b"JFIF\0");
                    l -= 5;
                    self.colour.jfif |= jfif;
                } else if m == 0xee && l >= 12 {
                    let adobe = tag(b"Adobe\0");
                    l -= 6;
                    if adobe {
                        // Version and two flag words, then the transform.
                        s.skip(5);
                        self.colour.app14_color_transform = i32::from(s.get8());
                        l -= 6;
                    }
                }
                s.skip(l);
                true
            }
            _ => false,
        }
    }

    /// `stbi__decode_jpeg_image` after the header: scans, `DNL` markers and
    /// other segments to the end-of-image marker, or to the first marker
    /// stb_image stops at.
    fn decode_image(&mut self, frame: &mut Frame, work: &mut Work) -> Result<Ending, ImageError> {
        let mut m = self.stream.get_marker();
        while m != 0xd9 {
            if m == 0xda {
                let scan = self.process_scan_header(frame).map_err(refused)?;
                self.parse_entropy_coded_data(frame, &scan, work)?;
                if self.stream.marker == NO_MARKER {
                    self.stream.marker = self.stream.skip_junk_at_end();
                }
                m = self.stream.get_marker();
                if (0xd0..=0xd7).contains(&m) {
                    m = self.stream.get_marker();
                }
            } else if m == 0xdc {
                let (ld, nl) = (self.stream.get16be(), self.stream.get16be());
                if ld != 4 || nl as usize != frame.height {
                    return Err(refused(Corrupt("bad DNL")));
                }
                m = self.stream.get_marker();
            } else {
                if !self.process_marker(m) {
                    return Ok(Ending::Stopped);
                }
                m = self.stream.get_marker();
            }
        }
        Ok(Ending::EndOfImage)
    }

    /// `stbi__process_scan_header`.
    fn process_scan_header(&mut self, frame: &Frame) -> Result<Scan, Corrupt> {
        let s = &mut self.stream;
        let ls = s.get16be();
        let scan_n = usize::from(s.get8());
        if !(1..=4).contains(&scan_n) || scan_n > frame.components.len() {
            return Err(Corrupt("bad SOS component count"));
        }
        if ls != 6 + 2 * scan_n as i32 {
            return Err(Corrupt("bad SOS len"));
        }
        let mut order = Vec::with_capacity(scan_n);
        for _ in 0..scan_n {
            let (id, q) = (s.get8(), s.get8());
            let which = frame
                .components
                .iter()
                .position(|c| c.id == id)
                .ok_or(Corrupt("a scan names a component the frame has not"))?;
            let (hd, ha) = (usize::from(q >> 4), usize::from(q & 15));
            if hd > 3 || ha > 3 {
                return Err(Corrupt("bad DC or AC huff"));
            }
            self.components[which].hd = hd;
            self.components[which].ha = ha;
            order.push(which);
        }
        let (start, end, aa) = (usize::from(s.get8()), usize::from(s.get8()), s.get8());
        let (high, low) = (u32::from(aa >> 4), u32::from(aa & 15));
        let band = if frame.progressive {
            if start > 63 || end > 63 || start > end || high > 13 || low > 13 {
                return Err(Corrupt("bad SOS"));
            }
            Band {
                start,
                end,
                high,
                low,
            }
        } else {
            if start != 0 || high != 0 || low != 0 {
                return Err(Corrupt("bad SOS"));
            }
            Band {
                start,
                end: 63,
                high,
                low,
            }
        };
        Ok(Scan { order, band })
    }

    /// `stbi__jpeg_reset`.
    fn reset(&mut self) {
        self.stream.reset_bits();
        for component in &mut self.components {
            component.dc_pred = 0;
        }
        self.todo = if self.restart_interval != 0 {
            self.restart_interval
        } else {
            i32::MAX
        };
        self.eob_run = 0;
    }

    /// The restart check after each MCU: true if the scan stops here, at
    /// the end of an interval with no restart marker after it.
    fn stops_here(&mut self) -> bool {
        self.todo -= 1;
        if self.todo <= 0 {
            if self.stream.code_bits() < 24 {
                self.stream.grow();
            }
            if !(0xd0..=0xd7).contains(&self.stream.marker) {
                return true;
            }
            self.reset();
        }
        false
    }

    /// `stbi__parse_entropy_coded_data`: every MCU of the scan, or those
    /// before the restart check stops it.
    fn parse_entropy_coded_data(
        &mut self,
        frame: &mut Frame,
        scan: &Scan,
        work: &mut Work,
    ) -> Result<(), ImageError> {
        // A scan of one component goes block by block over its own
        // samples; an interleaved one over the MCU grid.
        let one = if let [n] = scan.order[..] {
            let component = &frame.components[n];
            Some((n, component.width.div_ceil(8), component.height.div_ceil(8)))
        } else {
            None
        };
        let blocks = match one {
            Some((_, w, h)) => w * h,
            None => {
                let per_mcu: usize = scan
                    .order
                    .iter()
                    .map(|&n| frame.components[n].h * frame.components[n].v)
                    .sum();
                frame.mcus_wide * frame.mcus_high * per_mcu
            }
        };
        work.begin_scan(frame, self.stream.scan_data(), blocks as u64)?;
        self.reset();
        // The blocks walked: all of them, unless a restart interval with no
        // marker after it stops the scan early.
        let mut walked = 0u64;
        if let Some((n, w, h)) = one {
            'blocks: for j in 0..h {
                for i in 0..w {
                    if work.needs_block(self.stream.past_data()) {
                        work.block(self.stream.past_data(), self.stream.made_up_reads())?;
                    }
                    walked += 1;
                    self.decode_unit_block(frame, n, (i, j), scan.band, false)?;
                    if self.stops_here() {
                        break 'blocks;
                    }
                }
            }
        } else {
            let mcus = frame.mcus_wide * frame.mcus_high;
            for mcu in 0..mcus {
                let (i, j) = (mcu % frame.mcus_wide, mcu / frame.mcus_wide);
                for &n in &scan.order {
                    let (h, v) = (frame.components[n].h, frame.components[n].v);
                    for y in 0..v {
                        for x in 0..h {
                            let at = (i * h + x, j * v + y);
                            if work.needs_block(self.stream.past_data()) {
                                work.block(self.stream.past_data(), self.stream.made_up_reads())?;
                            }
                            walked += 1;
                            self.decode_unit_block(frame, n, at, scan.band, true)?;
                        }
                    }
                }
                if self.stops_here() {
                    break;
                }
            }
        }
        work.end_scan(walked, self.stream.made_up_reads())
    }

    /// One block of component `n` at `(bx, by)` in its blocks, as the scan
    /// decodes it: sequential, dequantised by the table in force now; a
    /// progressive DC pass, which every block of an interleaved scan is; or
    /// a progressive AC pass, refused if it refines a block not yet
    /// cleared.
    fn decode_unit_block(
        &mut self,
        frame: &mut Frame,
        n: usize,
        (bx, by): (usize, usize),
        band: Band,
        interleaved: bool,
    ) -> Result<(), ImageError> {
        // A band's first pass inside an end-of-band run touches nothing in
        // the block, as `decode_block_prog_ac` would find: the commonest
        // block of a progressive file, so it is passed before the block is
        // found.
        let first_pass = frame.progressive && !interleaved && band.start != 0 && band.high == 0;
        if first_pass && self.eob_run != 0 {
            self.eob_run -= 1;
            return Ok(());
        }
        let component = &mut frame.components[n];
        let block = by * component.blocks_wide + bx;
        let data: &mut [i16; 64] = (&mut component.coefficients[block * 64..block * 64 + 64])
            .try_into()
            .map_err(|_| malformed("a block outside its component"))?;
        // A sequential frame keeps no masks and never refines; its scratch
        // mask is written and dropped. A progressive one always has one a
        // block.
        let mut sequential_mask = 0;
        let nonzero = if frame.progressive {
            component
                .nonzero
                .get_mut(block)
                .ok_or_else(|| malformed("a progressive block without its nonzero mask"))?
        } else {
            &mut sequential_mask
        };
        let state = &mut self.components[n];
        let cleared = &mut self.cleared[n][block];
        if !frame.progressive {
            let tables = (&self.huff_dc[state.hd], &self.huff_ac[state.ha]);
            let dequant = &self.dequant[component.quant_index];
            entropy::decode_block(&mut self.stream, data, tables, &mut state.dc_pred, dequant)
                .map_err(refused)?;
            *cleared = true;
        } else if interleaved || band.start == 0 {
            let table = &self.huff_dc[state.hd];
            let coefficients = (data, nonzero);
            entropy::decode_block_prog_dc(
                &mut self.stream,
                coefficients,
                table,
                &mut state.dc_pred,
                band,
            )
            .map_err(refused)?;
            *cleared |= band.high == 0;
        } else {
            if band.high != 0 && !*cleared {
                return Err(uncleared("an AC refinement reading"));
            }
            let table = &self.huff_ac[state.ha];
            let coefficients = (data, nonzero);
            entropy::decode_block_prog_ac(
                &mut self.stream,
                coefficients,
                table,
                band,
                &mut self.eob_run,
            )
            .map_err(refused)?;
        }
        Ok(())
    }

    /// `stbi__jpeg_finish`'s dequantisation of a progressive frame, by the
    /// tables in force at its end, each product kept to 16 bits as
    /// stb_image's `short` keeps it.
    fn dequantize(&self, frame: &mut Frame) {
        for component in &mut frame.components {
            let dequant = &self.dequant[component.quant_index];
            for by in 0..component.height.div_ceil(8) {
                for bx in 0..component.width.div_ceil(8) {
                    let offset = (by * component.blocks_wide + bx) * 64;
                    let block = &mut component.coefficients[offset..offset + 64];
                    for (coefficient, &q) in block.iter_mut().zip(dequant) {
                        *coefficient = (i32::from(*coefficient) * i32::from(q)) as i16;
                    }
                }
            }
        }
    }
}
