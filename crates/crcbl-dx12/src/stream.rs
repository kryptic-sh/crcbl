//! Where each subobject of a D3D12 **pipeline state stream** lands, in bytes.
//!
//! # A mesh pipeline is not a struct, it is a packed stream
//!
//! `CreateGraphicsPipelineState` takes `D3D12_GRAPHICS_PIPELINE_STATE_DESC`, a
//! fixed struct with a slot for every stage D3D12 had in 2015 — and no slot for
//! an amplification or a mesh shader. The stages that arrived afterwards are
//! reachable only through `ID3D12Device2::CreatePipelineState`, whose
//! `D3D12_PIPELINE_STATE_STREAM_DESC` is a **pointer and a byte count**: a
//! caller packs the subobjects it wants, each tagged with its
//! `D3D12_PIPELINE_STATE_SUBOBJECT_TYPE`, and the runtime walks them.
//!
//! The C++ header ships `CD3DX12_PIPELINE_STATE_STREAM*` to do the packing with
//! `#pragma pack` and template glue. `windows-rs` ships no equivalent, so the
//! packing is this crate's, and it is arithmetic that type-checks whatever it
//! computes: every subobject is a `u32` tag followed by bytes, so a stream with
//! one field at the wrong offset is a stream the runtime reads a *different*
//! subobject out of.
//!
//! # The layout rule, which is the whole of this module
//!
//! Each subobject starts on an **8-byte boundary**, carries a `u32` type tag,
//! and then its data, aligned to the **data's own** alignment. Padding is zero.
//! That is the rule
//! <https://learn.microsoft.com/en-us/windows/win32/api/d3d12/ns-d3d12-d3d12_pipeline_state_stream_desc>
//! states and the rule `wgpu-hal`'s `dx12::pipeline_desc`'s `add_object`
//! implements; [`Stream::push`] is the same three steps, and
//! `the_layout_matches_a_known_good_packing` pins them against that
//! implementation's own byte-level test.
//!
//! Both alignments are **relative to the start of the buffer**, which is what
//! makes this arithmetic rather than pointer work — and is why nothing here has
//! to know where the allocation lives.
//!
//! # Not Windows-only, and that is the point
//!
//! This module holds no `windows` type — it is `usize` arithmetic over a
//! `Vec<u8>` — so off Windows it exists in the test build alone and `cargo test`
//! on any host checks it, exactly as [`crate::sync`], [`crate::resolve`] and
//! [`crate::dxil`] are compiled for. That matters more here than for any of
//! them: a stream packed wrong produces no compile error, no `HRESULT` a caller
//! can read, and a debug-layer message on a runner four minutes away.
//!
//! Writing the *data* is the caller's, through [`Stream::data_mut`], because the
//! bytes of a `D3D12_BLEND_DESC` only exist on the target that has one.

/// A pipeline state stream under construction.
///
/// Owns its bytes, because `D3D12_PIPELINE_STATE_STREAM_DESC` is a borrowed
/// pointer: the buffer must outlive the `CreatePipelineState` call, and holding
/// it in one value is what guarantees that without a lifetime an FFI struct can
/// express.
#[derive(Debug, Default)]
pub(crate) struct Stream {
    bytes: Vec<u8>,
}

impl Stream {
    /// An empty stream.
    pub(crate) const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Reserves one subobject and answers the byte offset its **data** starts
    /// at.
    ///
    /// `tag` is the subobject's `D3D12_PIPELINE_STATE_SUBOBJECT_TYPE` as a
    /// plain integer, `data_align` and `data_size` its payload's alignment and
    /// size. The reserved data is zeroed; the caller overwrites it through
    /// [`data_mut`](Self::data_mut).
    ///
    /// `data_align` is taken as the payload type's own `align_of`, so it is a
    /// power of two; a zero would be a caller that computed it rather than
    /// asked, and is treated as one byte rather than dividing by it.
    pub(crate) fn push(&mut self, tag: u32, data_align: usize, data_size: usize) -> usize {
        // The subobject itself, then the tag, then the payload — the three
        // steps in the order the format states them.
        self.pad_to(8);
        self.bytes.extend_from_slice(&tag.to_ne_bytes());
        self.pad_to(data_align.max(1));
        let at = self.bytes.len();
        self.bytes.resize(at + data_size, 0);
        at
    }

    /// The bytes one [`push`](Self::push) reserved, to write the payload into.
    ///
    /// # Panics
    ///
    /// If `at` and `len` do not name a region inside the stream — which cannot
    /// happen for the offset a `push` returned paired with the size it was
    /// given, and would be a caller pairing one subobject's offset with
    /// another's size.
    pub(crate) fn data_mut(&mut self, at: usize, len: usize) -> &mut [u8] {
        &mut self.bytes[at..at + len]
    }

    /// The whole stream, as `D3D12_PIPELINE_STATE_STREAM_DESC` takes it: a
    /// mutable pointer and a byte count.
    ///
    /// Mutable because the field is `*mut c_void`; the runtime reads it.
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    /// Zero-pads up to a multiple of `alignment`.
    fn pad_to(&mut self, alignment: usize) {
        let aligned = self.bytes.len().next_multiple_of(alignment);
        self.bytes.resize(aligned, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The packing, against a byte layout computed somewhere else.**
    ///
    /// The three subobjects, their tags and every offset below are `wgpu-hal`
    /// 30.0.0's `dx12::pipeline_desc`'s own `stream` test, which asserts the
    /// same layout over an independent implementation of the same rule. So this
    /// is a comparison against a second reading of the format rather than
    /// against this module's own arithmetic restated.
    ///
    /// Each of the three is a different shape of the rule: the `u16`'s data
    /// needs no padding after the tag and leaves the stream mid-boundary, the
    /// `u32`'s fills its subobject exactly, and the `u64`'s needs four bytes of
    /// padding between its tag and its data — the case a packer that only
    /// aligned subobject *starts* would get wrong, putting a pointer four bytes
    /// early and handing the runtime whatever followed it.
    #[test]
    fn the_layout_matches_a_known_good_packing() {
        let mut stream = Stream::new();

        let short = stream.push(1, align_of::<u16>(), size_of::<u16>());
        stream
            .data_mut(short, size_of::<u16>())
            .copy_from_slice(&42u16.to_ne_bytes());
        let word = stream.push(2, align_of::<u32>(), size_of::<u32>());
        stream
            .data_mut(word, size_of::<u32>())
            .copy_from_slice(&84u32.to_ne_bytes());
        let long = stream.push(3, align_of::<u64>(), size_of::<u64>());
        stream
            .data_mut(long, size_of::<u64>())
            .copy_from_slice(&168u64.to_ne_bytes());

        assert_eq!(short, 4, "the first tag is at zero and its data follows it");
        assert_eq!(
            word, 12,
            "the second subobject starts on the 8-byte boundary"
        );
        assert_eq!(long, 24, "an 8-aligned payload is pushed past its own tag");

        let bytes = stream.as_mut_slice();
        assert_eq!(bytes.len(), 32);
        assert_eq!(&bytes[0..4], &1u32.to_ne_bytes());
        assert_eq!(&bytes[4..6], &42u16.to_ne_bytes());
        assert_eq!(&bytes[6..8], &[0, 0], "padding is zero, not stale bytes");
        assert_eq!(&bytes[8..12], &2u32.to_ne_bytes());
        assert_eq!(&bytes[12..16], &84u32.to_ne_bytes());
        assert_eq!(&bytes[16..20], &3u32.to_ne_bytes());
        assert_eq!(&bytes[20..24], &[0, 0, 0, 0]);
        assert_eq!(&bytes[24..32], &168u64.to_ne_bytes());
    }

    /// **A subobject that ends four bytes short of a boundary pushes the next
    /// one out**, which is the case that tells 8-byte alignment from 4-byte.
    ///
    /// The shape is a real one: a payload of eight bytes aligned to four is
    /// `DXGI_SAMPLE_DESC`'s — two `u32`s — and its subobject is twelve bytes
    /// long, so every offset after it differs depending on which boundary the
    /// next subobject is aligned to. Red if [`Stream::push`] aligns to anything
    /// below 8, and the tags alone would not catch it: a stream of subobjects
    /// whose lengths happen to be multiples of eight packs identically either
    /// way.
    #[test]
    fn every_subobject_starts_on_an_eight_byte_boundary() {
        let mut stream = Stream::new();
        assert_eq!(
            stream.push(1, 4, 8),
            4,
            "a 4-aligned payload follows its tag"
        );
        assert_eq!(
            stream.as_mut_slice().len(),
            12,
            "the stream now ends mid-boundary"
        );

        assert_eq!(
            stream.push(2, 8, 8),
            24,
            "the second subobject's tag belongs at 16, not at 12"
        );
        assert_eq!(
            &stream.as_mut_slice()[12..16],
            &[0, 0, 0, 0],
            "the gap to the boundary is zero-padded"
        );
        assert_eq!(&stream.as_mut_slice()[16..20], &2u32.to_ne_bytes());
        assert_eq!(stream.as_mut_slice().len(), 32);
    }

    /// A subobject with no payload is a tag and nothing else — which is what a
    /// zero-sized reservation must be, rather than a byte of slack the next
    /// subobject is read out of.
    #[test]
    fn a_payload_of_no_bytes_reserves_none() {
        let mut stream = Stream::new();
        let empty = stream.push(9, 1, 0);
        assert_eq!(empty, 4);
        assert_eq!(stream.as_mut_slice().len(), 4);
        assert!(stream.data_mut(empty, 0).is_empty());
    }

    /// The payload is aligned to its **own** alignment, not to the tag's.
    ///
    /// Red if `push` reused the 8 it aligned the subobject with: a 2-aligned
    /// payload would then be pushed to offset 8 and every stream holding one
    /// would be four bytes longer than the runtime expects.
    #[test]
    fn the_payload_is_aligned_to_its_own_alignment() {
        for (align, expected) in [(1, 4), (2, 4), (4, 4), (8, 8)] {
            let mut stream = Stream::new();
            assert_eq!(
                stream.push(0, align, 8),
                expected,
                "a {align}-aligned payload"
            );
        }
        // And zero is read as one rather than divided by.
        let mut stream = Stream::new();
        assert_eq!(stream.push(0, 0, 4), 4);
    }

    /// Every byte a `push` reserves is zero before the caller writes it, so a
    /// subobject whose payload is only partly filled in carries defined bytes
    /// rather than whatever the allocation held.
    #[test]
    fn a_reservation_is_zeroed() {
        let mut stream = Stream::new();
        let at = stream.push(4, 8, 16);
        assert_eq!(stream.data_mut(at, 16), &[0u8; 16]);
    }
}
