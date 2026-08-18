//! Where an occlusion query lands in a Metal visibility-result buffer, and what
//! a range of them costs.
//!
//! # A Metal occlusion query set is a plain `MTLBuffer`
//!
//! Not an `MTLCounterSampleBuffer`. `MTLRenderPassDescriptor` carries a
//! `visibilityResultBuffer` and `MTLRenderCommandEncoder` carries
//! `setVisibilityResultMode:offset:`, whose header documents the offset as
//! "relative to the occlusion query buffer provided when the command encoder was
//! created" and requires it to be a multiple of eight — so the pool is ordinary
//! device memory, one `uint64_t` per query, and nothing about it goes through
//! `MTLDevice::counterSets`. That matters here rather than being trivia: the
//! machine CI runs this backend on advertises **no counter sets at all**, so the
//! two counter-sampled query kinds have no path on it while this one is
//! unconditional. `wgpu-hal`'s `metal` backend builds the same object, at the
//! same eight-byte stride (`wgpu_hal::QUERY_SIZE`).
//!
//! The eight-byte stride is therefore both Metal's result width *and* a legal
//! `setVisibilityResultMode:offset:` offset, which is what makes query `n`'s
//! slot addressable for every `n`. [`RESULT_BYTES`] is the number every size and
//! offset below is derived from.
//!
//! # `Counting`, not `Boolean`, when the seam grows a verb for it
//!
//! Nothing here calls `setVisibilityResultMode:offset:`, because
//! [`crcbl_hal::CommandEncoder`] has no begin/end query verb to call it from —
//! see `crate::device`'s `create_query_set`. When one arrives it should pass
//! `MTLVisibilityResultMode::Counting`, not `MTLVisibilityResultMode::Boolean`:
//! [`QueryKind::Occlusion`](crcbl_hal::QueryKind::Occlusion) is defined as
//! "samples that passed the depth test between begin and end", which is a count,
//! and crcbl's other two backends produce one — `crcbl-vk` sets
//! `occlusion_query_precise` on the pool it creates, and D3D12's
//! `D3D12_QUERY_TYPE_OCCLUSION` is the sample count rather than its
//! `BINARY_OCCLUSION` sibling. `wgpu-hal` picks `Boolean` for the opposite
//! reason and it is the right call *there*: WebGPU documents its occlusion
//! result as zero or one, so counting would be paying for precision the API
//! discards.
//!
//! # Not macOS-only, and that is the point
//!
//! There is no Objective-C in this file — it is `u64` arithmetic and one
//! [`HalError`] — so off macOS it is compiled under `cfg(test)` and `cargo test`
//! on any host checks it. `crate::quirk` and `crate::present` are the crate's
//! other modules of that shape and argue the split at length.

use crcbl_hal::HalError;

/// Bytes one occlusion query occupies in a visibility-result buffer.
///
/// Metal writes a `uint64_t` count per query, which is also the one `u64` per
/// query [`Device::query_results`](crcbl_hal::Device::query_results) and
/// [`resolve_query_set`](crcbl_hal::CommandEncoder::resolve_query_set) are both
/// shaped for — so unlike D3D12's pipeline-statistics struct, this kind needs no
/// width negotiation with the seam.
pub(crate) const RESULT_BYTES: u64 = size_of::<u64>() as u64;

/// The bytes a set of `count` queries occupies, or why there is no such set.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a count of zero:
/// `newBufferWithLength:options:` answers nil for a zero-length allocation, and
/// a set that did exist with no queries in it would be a handle whose every read
/// is out of range.
pub(crate) fn buffer_bytes(count: u32) -> Result<u64, HalError> {
    if count == 0 {
        return Err(HalError::InvalidDescriptor(
            "QuerySetDesc::count must be non-zero: MTLDevice::newBufferWithLength:options: \
             returns nil for a zero-length buffer, and every read of such a set would be out of \
             range"
                .to_string(),
        ));
    }
    Ok(span_bytes(u64::from(count)))
}

/// Bytes `queries` queries occupy, back to back.
///
/// Both the length of a resolve and the offset of query `n`, which is
/// `span_bytes(n)`.
///
/// No overflow check, and the caller is what rules one out: every count reaching
/// here has been through [`check_range`] against a `u32` query count, or is one
/// itself, so the widest product is `u32::MAX` strides and fits a `u64` with
/// room to spare.
pub(crate) const fn span_bytes(queries: u64) -> u64 {
    queries * RESULT_BYTES
}

/// Checks a read or a resolve against the set it names.
///
/// The bound [`Device::query_results`](crcbl_hal::Device::query_results)
/// documents — `first_query + queries` may not exceed the set — expressed in
/// `u64` so that a range near [`u32::MAX`] cannot wrap into a range that passes.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] naming both ends and the set's size.
pub(crate) fn check_range(count: u32, first_query: u32, queries: u64) -> Result<(), HalError> {
    let end = u64::from(first_query) + queries;
    if end > u64::from(count) {
        return Err(HalError::InvalidDescriptor(format!(
            "query range {first_query}..{end} exceeds the set's {count} queries"
        )));
    }
    Ok(())
}

/// Checks a resolve's destination against its own size.
///
/// Metal bounds-checks neither end of
/// `copyFromBuffer:sourceOffset:toBuffer:destinationOffset:size:` — `objc2`
/// marks the selector unsafe for exactly that — and an overrun is a raise that
/// aborts the process rather than an error a caller can catch. So the span is
/// checked here, while it is still one.
///
/// `queries` must already have been through [`check_range`]; see
/// [`span_bytes`].
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a span that overflows a `u64`, or for one
/// that runs past `dst_size`.
pub(crate) fn check_destination(
    dst_offset: u64,
    queries: u64,
    dst_size: u64,
) -> Result<(), HalError> {
    let end = dst_offset.checked_add(span_bytes(queries)).ok_or_else(|| {
        HalError::InvalidDescriptor(format!(
            "resolving {queries} occlusion queries at offset {dst_offset} overflows a u64"
        ))
    })?;
    if end > dst_size {
        return Err(HalError::InvalidDescriptor(format!(
            "resolve_query_set writes {dst_offset}..{end} of a {dst_size}-byte buffer; Metal \
             resolves {RESULT_BYTES} bytes per occlusion query"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A set of no queries is refused, and a set of some sizes to the stride.
    ///
    /// Red if the zero case is dropped: `newBufferWithLength:0` answers nil,
    /// which this backend reports as [`HalError::OutOfDeviceMemory`] — "the GPU
    /// is out of memory" for a descriptor that was never legal.
    #[test]
    fn a_query_set_of_zero_queries_has_no_visibility_buffer() {
        assert_eq!(
            buffer_bytes(4).expect("four queries"),
            4 * size_of::<u64>() as u64
        );
        assert_eq!(buffer_bytes(1).expect("one query"), size_of::<u64>() as u64);

        let error = buffer_bytes(0).expect_err("no queries");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // Query `n` starts where the previous one ended, which is the whole of
        // what makes a set a flat array rather than a Metal object.
        assert_eq!(span_bytes(0), 0);
        assert_eq!(span_bytes(3), 3 * RESULT_BYTES);
    }

    /// **The bound is `first + len <= count`, and it is evaluated in `u64`.**
    ///
    /// Red if the addition is done in `u32`: `check_range(2, u32::MAX, 2)` wraps
    /// to `1`, which is inside a two-query set, so a read one past the end — the
    /// pair `crcbl`'s seam suite checks — would be accepted on exactly the input
    /// a caller is most likely to have arrived at by arithmetic.
    #[test]
    fn a_read_past_the_end_of_a_set_is_refused_and_the_bound_cannot_wrap() {
        check_range(2, 0, 2).expect("the whole set");
        check_range(2, 1, 1).expect("the last query");
        check_range(2, 2, 0).expect("an empty read at the end is inside the set");

        let error = check_range(2, 0, 3).expect_err("one past the end");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let text = error.to_string();
        assert!(text.contains('3') && text.contains('2'), "{text}");

        assert!(
            check_range(2, u32::MAX, 2).is_err(),
            "the range wrapped and a read far past the end was accepted"
        );
    }

    /// A resolve destination must hold what Metal is about to write into it.
    ///
    /// Red if the check is dropped or done in the wrong width: without it a
    /// two-query resolve into an eight-byte buffer reaches
    /// `copyFromBuffer:…:size:`, which bounds-checks nothing and raises — and a
    /// Metal raise aborts the process rather than failing the call.
    #[test]
    fn a_resolve_destination_must_hold_every_query_it_is_given() {
        check_destination(0, 2, 16).expect("exactly two queries");
        check_destination(8, 1, 16).expect("the second slot");
        check_destination(16, 0, 16).expect("an empty resolve at the end fits");

        let error = check_destination(8, 2, 16).expect_err("one query too many");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(error.to_string().contains("24"), "{error}");

        let error = check_destination(0, 2, 8).expect_err("half a destination");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // Overflow is refused rather than wrapping into a range that fits.
        let error = check_destination(u64::MAX - 7, 2, u64::MAX).expect_err("wraps");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }
}
