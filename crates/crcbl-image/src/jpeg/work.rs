//! What entropy decoding may spend for the data a JPEG brings, for both
//! decoders; the [module docs](super#work-bounded-by-the-input) say why.
//!
//! Work done from real data bounds itself: each Huffman code, magnitude bit
//! and correction bit decoded from the file is at least one of its bits, so
//! there are at most eight a byte, and each costs a fixed amount. What does
//! not is work done without bits: blocks an end-of-band run skips, and
//! reads of bits a decoder makes up past the end of a scan's data (both
//! decoders read zeros there). Those are what [`Work`] counts, against the
//! entropy-coded bytes the scans brought and the frame's blocks.
//!
//! A read is a Huffman code with the magnitude or run bits that follow it,
//! or a lone sign, correction or refinement bit. It is made up if any of
//! its bits was, or if a decoder gave its value without bits (stb_image
//! reads 0 for a field the buffer cannot hold). Both decoders count reads
//! the same way.
//!
//! In its end-of-band tails, where the scan's data says only that the rest
//! of a band is refined, a refinement visits only the coefficients it
//! corrects (each block's nonzero mask finds them), so walking a block
//! costs about the same there as in every other kind of scan, and one
//! count of blocks covers them all. Elsewhere it passes at most a run's
//! zero coefficients between one read and the next.

use super::{Frame, past_fill};
use crate::{ImageError, WorkBound};

/// The most scans a JPEG may have, beyond which both decoders refuse it as
/// [`ImageError::TooMuchWork`]: what jpegtran's scan scripts allow
/// (`MAX_SCANS` in libjpeg-turbo's `rdswitch.c`), one more than Blink's
/// progress monitor lets Chrome decode, the default of zune-jpeg's
/// `max_scans`, and what libjpeg-turbo's own fuzz targets set
/// `TJPARAM_SCANLIMIT` to for lossy images.
pub const MAX_SCANS: u32 = 100;

/// The most blocks the scans may walk for each byte of entropy-coded data
/// they brought, beyond one walk over the frame's blocks that a file is
/// allowed once.
///
/// A real file stays under it: each component's first DC pass (or its
/// sequential scan) codes every block it covers, a Huffman code is at least
/// one bit, so the bytes are at least an eighth of the blocks that pass
/// walks, and no component is in more than [`MAX_SCANS`] scans. The one
/// walk over the frame is for a file whose data runs out early, which both
/// decoders read on as zeros; it is no more than the stages after decoding
/// spend on every frame anyway.
pub(crate) const BLOCKS_PER_ENTROPY_BYTE: u64 = 8 * MAX_SCANS as u64;

/// The most made-up reads for each block of the frame, over the whole file:
/// enough for every block to be read once, at the most one pass can read
/// in a block, from made-up bits, which is what a file cut short needs to
/// finish.
///
/// A sequential block takes a code for its DC and one for each of its 63
/// AC coefficients: 64. A progressive first pass takes no more. A
/// refinement pass takes the most: for each of the 63 AC coefficients
/// either a code and a sign bit (a new coefficient) or a correction bit (a
/// nonzero one), and an end-of-band code: 127, and a DC refinement bit
/// lies in a scan of its own.
pub(crate) const PADDED_READS_PER_BLOCK: u64 = 128;

/// The allowance for a file damaged in more than one scan, each reading
/// the rest of itself from made-up bits: the most made-up reads for each
/// byte of entropy-coded data the scans brought, besides
/// [`PADDED_READS_PER_BLOCK`] for each block. Real data gives at most eight
/// reads a byte, a bit each, so this lets made-up bits cost at most eight
/// times what the same bytes could. It is the one knob for that policy.
pub(crate) const PADDED_READS_PER_ENTROPY_BYTE: u64 = 64;

/// The work entropy decoding has done so far, checked as it goes.
#[derive(Debug, Default)]
pub(super) struct Work {
    /// The scans begun.
    pub(super) scans: u32,
    /// The entropy-coded bytes the scans begun brought.
    pub(super) entropy_bytes: u64,
    /// The blocks the scans walked.
    pub(super) walked: u64,
    /// The made-up reads, over every scan.
    pub(super) padded_reads: u64,
    /// The most blocks and made-up reads allowed, as of the scan in
    /// progress: computed when it begins, so each check is a comparison.
    walk_limit: u64,
    read_limit: u64,
    /// Whether the scan in progress is charged block by block, because it
    /// might walk past `walk_limit`; otherwise it is charged what it walked
    /// when it ends.
    per_block: bool,
    /// The frame's size, which a refusal names.
    width: u32,
    height: u32,
}

impl Work {
    /// Accounts for a scan of `frame` starting, `blocks` blocks long, its
    /// entropy-coded data at `start` in `data`. Refused once there are more
    /// than [`MAX_SCANS`]. Its blocks are charged one by one only when they
    /// might not all fit what the data allows; a scan that fits is charged
    /// what it walked when it ends.
    pub(super) fn begin_scan(
        &mut self,
        frame: &Frame,
        (data, start): (&[u8], usize),
        blocks: u64,
    ) -> Result<(), ImageError> {
        let frame_blocks = frame
            .components
            .iter()
            .map(|c| (c.blocks_wide as u64).saturating_mul(c.blocks_high as u64))
            .fold(0, u64::saturating_add);
        self.width = frame.width as u32;
        self.height = frame.height as u32;
        self.scans = self.scans.saturating_add(1);
        self.entropy_bytes = self
            .entropy_bytes
            .saturating_add(entropy_coded_len(data, start) as u64);
        if self.scans > MAX_SCANS {
            return Err(self.refusal(WorkBound::Scans { scans: self.scans }));
        }
        // Saturating, but never near it: the bytes are no more than the
        // input's length, and the frame's blocks no more than the
        // coefficient store its limits allow, so the products stay far
        // below `u64::MAX` and the limits stay finite.
        self.walk_limit =
            frame_blocks.saturating_add(self.entropy_bytes.saturating_mul(BLOCKS_PER_ENTROPY_BYTE));
        self.read_limit = frame_blocks
            .saturating_mul(PADDED_READS_PER_BLOCK)
            .saturating_add(
                self.entropy_bytes
                    .saturating_mul(PADDED_READS_PER_ENTROPY_BYTE),
            );
        self.per_block = self.walked.saturating_add(blocks) > self.walk_limit;
        Ok(())
    }

    /// Accounts for a block of the scan in progress about to be walked,
    /// with `padded_reads` made up over the whole file so far, `past_data`
    /// if the decoder's reader has reached the end of the scan's data. A
    /// scan charged block by block is charged it and checked; one that fits
    /// has its reads checked only while its reader is past its data, where
    /// they are made up. A restart starts the reader on the next interval's
    /// data, so reads made up at the end of an interval of a scan that fits
    /// are checked at the next block past data, or at
    /// [`end_scan`](Self::end_scan): such a file can be refused at the end
    /// of the scan rather than in its course.
    #[inline]
    pub(super) fn block(&mut self, past_data: bool, padded_reads: u64) -> Result<(), ImageError> {
        if self.per_block {
            self.walk(padded_reads)
        } else if past_data {
            self.check_reads(padded_reads)
        } else {
            Ok(())
        }
    }

    /// Whether a block of the scan in progress needs [`block`](Self::block)
    /// called for it: always in a scan charged block by block, and in one
    /// that fits only while the reader is past its data. Checked first so a
    /// scan that fits spends nothing a block on the bound.
    #[inline]
    pub(super) fn needs_block(&self, past_data: bool) -> bool {
        self.per_block || past_data
    }

    /// The made-up reads recorded so far, over every scan before this one.
    pub(super) fn padded_reads(&self) -> u64 {
        self.padded_reads
    }

    /// Charges a block about to be walked, in a scan charged block by block,
    /// with `padded_reads` made up over the whole file so far: refused once
    /// either has passed what the data allows.
    #[inline]
    fn walk(&mut self, padded_reads: u64) -> Result<(), ImageError> {
        self.walked = self.walked.saturating_add(1);
        if self.walked > self.walk_limit {
            return Err(self.refusal(WorkBound::BlocksWalked {
                entropy_bytes: self.entropy_bytes,
            }));
        }
        self.check_reads(padded_reads)
    }

    /// Checks `padded_reads` made up over the whole file so far: refused
    /// once they have passed what the data allows.
    #[inline]
    fn check_reads(&mut self, padded_reads: u64) -> Result<(), ImageError> {
        self.padded_reads = padded_reads;
        if self.padded_reads > self.read_limit {
            return Err(self.refusal(WorkBound::MadeUpReads {
                entropy_bytes: self.entropy_bytes,
            }));
        }
        Ok(())
    }

    /// Accounts for the scan in progress ending, having walked `walked`
    /// blocks, with `padded_reads` made up over the whole file: charged the
    /// blocks if it was not charged them one by one, and checked.
    pub(super) fn end_scan(&mut self, walked: u64, padded_reads: u64) -> Result<(), ImageError> {
        if !self.per_block {
            // It fits: `begin_scan` found all its blocks within the limit.
            self.walked = self.walked.saturating_add(walked);
        }
        self.check_reads(padded_reads)
    }

    fn refusal(&self, bound: WorkBound) -> ImageError {
        ImageError::TooMuchWork {
            width: self.width,
            height: self.height,
            bound,
        }
    }
}

/// How many bytes of entropy-coded data a scan has from `start`: up to the
/// first marker that is not a restart marker, past stuffed zeros and fill
/// bytes, or to the end of the file.
pub(super) fn entropy_coded_len(data: &[u8], start: usize) -> usize {
    let mut pos = start;
    // Straight to each 0xff, the only byte that can start a marker.
    while let Some(at) = next_0xff(data, pos) {
        let after_fill = past_fill(data, at);
        match data.get(after_fill + 1) {
            Some(0x00 | 0xd0..=0xd7) => pos = after_fill + 2,
            Some(_) => return at - start,
            None => break,
        }
    }
    data.len().saturating_sub(start)
}

/// Where the first 0xff at or after `from` is. Chunks are tested whole,
/// with no early exit, which the compiler turns into a few vector compares,
/// many times quicker than looking at each byte in turn.
fn next_0xff(data: &[u8], from: usize) -> Option<usize> {
    let mut offset = from;
    for chunk in data.get(from..)?.chunks(SKIP_CHUNK) {
        if chunk
            .iter()
            .fold(false, |found, &byte| found | (byte == 0xff))
        {
            return chunk
                .iter()
                .position(|&byte| byte == 0xff)
                .map(|at| offset + at);
        }
        offset += chunk.len();
    }
    None
}

/// How many bytes [`next_0xff`] tests at once.
const SKIP_CHUNK: usize = 32;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Limits;
    use crate::jpeg::read_frame;

    /// An 8 × 8 grey frame: one block.
    fn one_block() -> Frame {
        read_frame(&[8, 0, 8, 0, 8, 1, 1, 0x11, 0], false, &Limits::DEFAULT).expect("a frame")
    }

    #[test]
    fn the_constants_are_their_sources() {
        // jpegtran's script cap; one bit a block: eight blocks a byte for
        // each scan.
        assert_eq!((MAX_SCANS, BLOCKS_PER_ENTROPY_BYTE), (100, 800));
        // A refinement pass's 127 reads a block, rounded up; eight times
        // the bits a byte.
        assert_eq!(
            (PADDED_READS_PER_BLOCK, PADDED_READS_PER_ENTROPY_BYTE),
            (128, 64)
        );
    }

    /// One free walk over the frame, then [`BLOCKS_PER_ENTROPY_BYTE`] for
    /// each byte, and not a block more, charged block by block when a scan
    /// might not fit.
    #[test]
    fn blocks_walked_are_one_frame_and_a_share_for_each_byte() {
        let frame = one_block();
        let mut work = Work::default();
        work.begin_scan(
            &frame,
            (&[0x12, 0xff, 0xd9], 0),
            2 + BLOCKS_PER_ENTROPY_BYTE,
        )
        .expect("a scan");
        assert!(work.per_block);
        for _ in 0..1 + BLOCKS_PER_ENTROPY_BYTE {
            work.block(false, 0).expect("within the bound");
        }
        assert!(matches!(
            work.block(false, 0),
            Err(ImageError::TooMuchWork {
                bound: WorkBound::BlocksWalked { entropy_bytes: 1 },
                ..
            })
        ));
        let mut work = Work::default();
        work.begin_scan(&frame, (&[0xff, 0xd9], 0), 2)
            .expect("a scan");
        work.block(false, 0).expect("the free walk");
        assert!(work.block(false, 0).is_err());
    }

    /// A scan whose blocks all fit is charged them at its end, and only
    /// those it walked.
    #[test]
    fn a_scan_that_fits_is_charged_what_it_walked_at_its_end() {
        let frame = one_block();
        let mut work = Work::default();
        work.begin_scan(
            &frame,
            (&[0x12, 0xff, 0xd9], 0),
            1 + BLOCKS_PER_ENTROPY_BYTE,
        )
        .expect("a scan");
        assert!(!work.per_block);
        // Not charged block by block, nor checked before its reader is
        // past its data.
        work.block(false, u64::MAX).expect("not checked");
        assert_eq!(work.walked, 0);
        work.end_scan(5, 0).expect("within the bound");
        assert_eq!(work.walked, 5);
    }

    /// [`PADDED_READS_PER_BLOCK`] for each block of the frame and
    /// [`PADDED_READS_PER_ENTROPY_BYTE`] for each byte, over the file, and
    /// not a read more.
    #[test]
    fn reads_of_made_up_bits_are_a_share_for_each_block_and_byte() {
        let frame = one_block();
        let mut work = Work::default();
        work.begin_scan(&frame, (&[0x12, 0x34, 0xff, 0xd9], 0), 1)
            .expect("a scan");
        let allowed = PADDED_READS_PER_BLOCK + 2 * PADDED_READS_PER_ENTROPY_BYTE;
        work.block(true, allowed).expect("within the bound");
        assert!(matches!(
            work.block(true, allowed + 1),
            Err(ImageError::TooMuchWork {
                bound: WorkBound::MadeUpReads { entropy_bytes: 2 },
                ..
            })
        ));
        assert!(work.end_scan(1, allowed + 1).is_err());
    }

    #[test]
    fn the_scan_after_the_last_allowed_is_refused() {
        let frame = one_block();
        let mut work = Work::default();
        for _ in 0..MAX_SCANS {
            work.begin_scan(&frame, (&[0x12], 0), 1).expect("a scan");
        }
        assert!(matches!(
            work.begin_scan(&frame, (&[0x12], 0), 1),
            Err(ImageError::TooMuchWork {
                bound: WorkBound::Scans { scans: 101 },
                ..
            })
        ));
    }

    /// Each bound's refusal says which it was, truly.
    #[test]
    fn each_refusal_names_its_bound() {
        let refusal = |bound| {
            ImageError::TooMuchWork {
                width: 8,
                height: 8,
                bound,
            }
            .to_string()
        };
        let prefix = "a 8x8 JPEG is too much work to decode: ";
        for (bound, text) in [
            (
                WorkBound::Scans { scans: 101 },
                "it has 101 scans, more than an encoder writes",
            ),
            (
                WorkBound::BlocksWalked { entropy_bytes: 3 },
                "its scans would walk more blocks than their 3 bytes of entropy-coded data \
                 account for",
            ),
            (
                WorkBound::MadeUpReads { entropy_bytes: 3 },
                "its scans would make more reads of bits from past the end of their data \
                 than its frame and their 3 bytes of entropy-coded data allow",
            ),
        ] {
            assert_eq!(refusal(bound), format!("{prefix}{text}"));
        }
    }

    /// A scan's data runs to the first marker that is not a restart
    /// marker, stuffed zeros and fill bytes included, or to the end.
    #[test]
    fn entropy_coded_data_runs_to_the_next_marker_past_restarts() {
        for (data, len) in [
            (&b"\x01\x02\xff\xd9"[..], 2),
            (b"\x01\xff\x00\x02\xff\xd9", 4),
            (b"\x01\xff\xff\x00\x02\xff\xd9", 5),
            (b"\x01\xff\xd0\x02\xff\xd7\x03\xff\xc4", 7),
            (b"\x01\xff\xff\xda", 1),
            (b"\x01\x02\x03", 3),
            (b"\x01\xff", 2),
            (b"", 0),
        ] {
            assert_eq!(entropy_coded_len(data, 0), len, "{data:02x?}");
        }
        assert_eq!(entropy_coded_len(b"\x01", 5), 0, "a start past the end");
    }
}
