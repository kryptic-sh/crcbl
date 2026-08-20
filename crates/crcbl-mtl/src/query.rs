//! What a Metal query result is worth, where one lands, and what a range of
//! them costs.
//!
//! # Two objects, because Metal has two
//!
//! An **occlusion** query set is a plain `MTLBuffer` — the one a render pass
//! names through `MTLRenderPassDescriptor::visibilityResultBuffer`, with
//! `MTLRenderCommandEncoder::setVisibilityResultMode:offset:` writing into it,
//! whose header documents the offset as "relative to the occlusion query buffer
//! provided when the command encoder was created" and requires it to be a
//! multiple of eight. So that pool is ordinary device memory, one `uint64_t` per
//! query, and nothing about it goes through `MTLDevice::counterSets`.
//! `wgpu-hal`'s `metal` backend builds the same object at the same eight-byte
//! stride (`wgpu_hal::QUERY_SIZE`).
//!
//! The other two kinds are `MTLCounterSampleBuffer`s built over a set from
//! `MTLDevice::counterSets`, and they resolve at a **width the counter set
//! decides**: a timestamp is one `MTLCounterResultTimestamp`, which is one
//! `u64`, and a statistics query is one `MTLCounterResultStatistic`, which is
//! [`STATISTIC_COUNTERS`] of them. [`result_bytes`] is the number every size,
//! offset and bound below is derived from, and `crate::conv` holds the
//! compile-time assertions tying each figure to the Metal structure it claims
//! to be — so the literals here cannot drift on an `objc2-metal` upgrade, and
//! everything derived from them is checked on a machine with no Metal at all.
//!
//! # Not macOS-only, and that is the point
//!
//! There is no Objective-C in this file — it is `u64` arithmetic and
//! [`HalError`] — so off macOS it is compiled under `cfg(test)` and `cargo test`
//! on any host checks it. `crate::pass`, `crate::quirk` and `crate::present` are
//! the crate's other modules of that shape and argue the split at length. It
//! matters most for [`timestamp_nanos`], which is the one piece of the timestamp
//! path that is arithmetic rather than a message send: every other line of it
//! needs a Mac to execute, and this one does not.
//!
//! # `Counting`, not `Boolean`, when the seam grows a verb for it
//!
//! Nothing here calls `setVisibilityResultMode:offset:`, because
//! [`crcbl_hal::CommandEncoder`] has no begin/end query verb to call it from —
//! see `crate::device`'s `create_query_set`. When one arrives it should pass
//! `MTLVisibilityResultMode::Counting`, not `MTLVisibilityResultMode::Boolean`:
//! [`QueryKind::Occlusion`] is defined as "samples that passed the depth test
//! between begin and end", which is a count, and crcbl's other two backends
//! ask for one — D3D12's `D3D12_QUERY_TYPE_OCCLUSION` is the sample count
//! rather than its `BINARY_OCCLUSION` sibling, and `crcbl-vk`'s
//! `VK_QUERY_TYPE_OCCLUSION` pool yields one once a begin can pass
//! `VK_QUERY_CONTROL_PRECISE_BIT`, which is blocked on the same missing verb
//! this section is about. `wgpu-hal` picks `Boolean` for the
//! opposite reason and it is the right call *there*: WebGPU documents its
//! occlusion result as zero or one, so counting would be paying for precision
//! the API discards.

use crcbl_hal::{HalError, PassTimestampWrites, QueryKind};

/// Counters an `MTLCounterResultStatistic` holds.
///
/// Fixed by Metal's ABI — `tessellationInputPatches` through
/// `computeKernelInvocations` — rather than chosen here, which is why it is a
/// literal at all. `crate::conv` holds the compile-time assertion that ties
/// [`result_bytes`] to the bindings' own structure.
pub(crate) const STATISTIC_COUNTERS: u64 = 8;

/// The value Metal writes for a sample that was never taken or failed to
/// resolve.
///
/// `MTLCounterErrorValue`, spelled here because this module is compiled where
/// `objc2-metal` is not; `crate::conv` asserts the two are equal. It is a
/// *sentinel inside a `u64` result*, so a reader that did not know about it
/// would report `u64::MAX` nanoseconds for a pass nothing sampled —
/// [`timestamp_nanos`] maps it to zero, which is what the seam's degrading rule
/// asks a backend to return for a timing it does not have.
pub(crate) const COUNTER_ERROR: u64 = u64::MAX;

/// Bytes one resolved query of this kind occupies.
///
/// The stride `MTLBlitCommandEncoder`'s
/// `resolveCounters:inRange:destinationBuffer:destinationOffset:` writes with —
/// it takes no stride parameter, exactly as D3D12's `ResolveQueryData` does not
/// — and so the size of a resolve destination, the offset of a query within
/// one, and the bound a caller's buffer is checked against.
pub(crate) const fn result_bytes(kind: QueryKind) -> u64 {
    match kind {
        // A visibility-result slot is a `uint64_t` sample count and an
        // `MTLCounterResultTimestamp` is a single `u64` `timestamp` field, which
        // is the one `u64` per query the seam's two read paths are shaped for.
        QueryKind::Occlusion | QueryKind::Timestamp => size_of::<u64>() as u64,
        QueryKind::PipelineStatistics => STATISTIC_COUNTERS * size_of::<u64>() as u64,
    }
}

/// Whether a set of this many queries can exist at all.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a count of zero. Both objects refuse it
/// and the seam refuses it for a third reason of its own:
/// `newBufferWithLength:options:` answers nil for a zero-length allocation, an
/// `MTLCounterSampleBufferDescriptor` with `sampleCount` zero has no sample to
/// address, and a set that did exist with no queries in it would be a handle
/// whose every read is out of range.
pub(crate) fn check_count(count: u32) -> Result<(), HalError> {
    if count == 0 {
        return Err(HalError::InvalidDescriptor(
            "QuerySetDesc::count must be non-zero: MTLDevice::newBufferWithLength:options: \
             returns nil for a zero-length buffer, an MTLCounterSampleBuffer of no samples has \
             nothing to sample into, and every read of such a set would be out of range"
                .to_string(),
        ));
    }
    Ok(())
}

/// The bytes an occlusion set of `count` queries occupies, or why there is no
/// such set.
///
/// Only the occlusion kind has one: the counter-sampled kinds are an
/// `MTLCounterSampleBuffer` sized in *samples* rather than an allocation this
/// backend measures in bytes.
///
/// # Errors
///
/// As [`check_count`].
pub(crate) fn buffer_bytes(count: u32) -> Result<u64, HalError> {
    check_count(count)?;
    Ok(span_bytes(QueryKind::Occlusion, u64::from(count)))
}

/// Bytes `queries` queries of this kind occupy, back to back.
///
/// Both the length of a resolve and the offset of query `n`, which is
/// `span_bytes(kind, n)`.
///
/// No overflow check, and the caller is what rules one out: every count reaching
/// here has been through [`check_range`] against a `u32` query count, or is one
/// itself, so the widest product is `u32::MAX` strides and fits a `u64` with
/// room to spare.
pub(crate) const fn span_bytes(kind: QueryKind, queries: u64) -> u64 {
    queries * result_bytes(kind)
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
/// `copyFromBuffer:sourceOffset:toBuffer:destinationOffset:size:` nor of
/// `resolveCounters:inRange:destinationBuffer:destinationOffset:` — `objc2`
/// marks both selectors unsafe for exactly that — and an overrun is a raise that
/// aborts the process rather than an error a caller can catch. So the span is
/// checked here, while it is still one.
///
/// **The width is the kind's**, which is what makes this more than a bounds
/// check: a caller that sized a destination for one `u64` per query gets an
/// error naming the real stride rather than a resolve that writes
/// [`STATISTIC_COUNTERS`] times as far as it expected.
///
/// `queries` must already have been through [`check_range`]; see
/// [`span_bytes`].
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a span that overflows a `u64`, or for one
/// that runs past `dst_size`.
pub(crate) fn check_destination(
    kind: QueryKind,
    dst_offset: u64,
    queries: u64,
    dst_size: u64,
) -> Result<(), HalError> {
    let end = dst_offset
        .checked_add(span_bytes(kind, queries))
        .ok_or_else(|| {
            HalError::InvalidDescriptor(format!(
                "resolving {queries} {kind:?} queries at offset {dst_offset} overflows a u64"
            ))
        })?;
    if end > dst_size {
        return Err(HalError::InvalidDescriptor(format!(
            "resolve_query_set writes {dst_offset}..{end} of a {dst_size}-byte buffer; Metal \
             resolves {} bytes per {kind:?} query and takes no stride to narrow that with",
            result_bytes(kind)
        )));
    }
    Ok(())
}

/// Checks the destination offset of a **counter** resolve against Metal's
/// alignment rule.
///
/// `resolveCounters:inRange:destinationBuffer:destinationOffset:` documents its
/// offset as "a multiple of the minimum constant buffer alignment", which
/// nothing in `MTLDevice` will answer. What this backend can answer is the
/// number it *promises*: [`Limits::min_uniform_buffer_offset_alignment`](crcbl_hal::Limits::min_uniform_buffer_offset_alignment),
/// which `crcbl_mtl::adapter` leaves at the seam's floor and therefore
/// guarantees, and which a caller binding a uniform buffer already honours.
/// Passing it in rather than spelling a constant here is what keeps the two the
/// same number.
///
/// The blit *copy* an occlusion resolve records has no such rule and is not put
/// through this.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] naming the offset and the alignment.
pub(crate) fn check_resolve_alignment(dst_offset: u64, alignment: u64) -> Result<(), HalError> {
    if alignment != 0 && !dst_offset.is_multiple_of(alignment) {
        return Err(HalError::InvalidDescriptor(format!(
            "resolve_query_set's dst_offset is {dst_offset}, and \
             resolveCounters:inRange:destinationBuffer:destinationOffset: requires a multiple of \
             the minimum constant buffer alignment, which this device reports as {alignment}"
        )));
    }
    Ok(())
}

/// Checks the pair of queries a pass named for its timestamps.
///
/// Every rule here is the seam's rather than Metal's, and each would otherwise
/// surface far from the descriptor that caused it: a pair that names one query
/// twice measures nothing at all, and an index past the end of the set reaches
/// `setStartOfVertexSampleIndex:` — which `objc2` marks unsafe because Metal
/// does not bounds-check it.
///
/// The kind check is the seam's too:
/// [`PassTimestampWrites::set`] "must hold [`QueryKind::Timestamp`] queries; a
/// backend fails the encoder rather than writing a timestamp into a pool of
/// another kind". On this backend the wrong kind is also a different *object* —
/// an occlusion pool is an `MTLBuffer` and has no `sampleBuffer` to attach — so
/// it could not be honoured even if the seam allowed it.
///
/// `what` names the verb, so the message says which call was malformed.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a set of the wrong kind, for a pair that
/// names one query twice, or for an index the set does not hold.
pub(crate) fn check_timestamp_pair(
    what: &str,
    writes: &PassTimestampWrites,
    kind: QueryKind,
    count: u32,
) -> Result<(), HalError> {
    if kind != QueryKind::Timestamp {
        return Err(HalError::InvalidDescriptor(format!(
            "{what} names a {kind:?} query set for its timestamps; a timestamp is written into a \
             QueryKind::Timestamp set"
        )));
    }
    if writes.beginning_of_pass == writes.end_of_pass {
        return Err(HalError::InvalidDescriptor(format!(
            "{what} writes both of its timestamps into query {}; the two must be distinct queries \
             or the pass measures nothing",
            writes.beginning_of_pass
        )));
    }
    for index in [writes.beginning_of_pass, writes.end_of_pass] {
        if index >= count {
            return Err(HalError::InvalidDescriptor(format!(
                "{what} writes a timestamp into query {index} of a {count}-query set"
            )));
        }
    }
    Ok(())
}

/// One reading of the CPU and GPU clocks, as
/// `MTLDevice::sampleTimestamps:gpuTimestamp:` hands them back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Correlation {
    /// The host clock, in nanoseconds.
    pub(crate) cpu: u64,
    /// The GPU clock, in whatever that device counts in.
    pub(crate) gpu: u64,
}

/// A raw Metal GPU timestamp on the host's nanosecond clock, which is what
/// [`Device::query_results`](crcbl_hal::Device::query_results) reports.
///
/// # Metal states no period at all, so two correlations are the measurement
///
/// Vulkan reports nanoseconds per tick and D3D12 reports its reciprocal; Metal
/// reports neither, because `sampleTimestamps:gpuTimestamp:` correlates the two
/// clocks at the moment of asking rather than promising a fixed rate. **Two such
/// readings are what a rate is derived from**, and that is Apple's own
/// documented conversion (the "Measuring Performance Using GPU Counters" sample
/// brackets the work with a pair and scales by `cpuSpan / gpuSpan`) rather than
/// something invented here. `wgpu-hal` 30.0.0's `src/metal/adapter.rs` instead
/// picks `83.333` when the device name starts with `Intel` and `1.0` otherwise,
/// and its own comment calls that "the dangerous but easy thing".
///
/// `base` is taken once when the device opens and `now` at the read, so the
/// window is every nanosecond the device has been alive — which is what makes
/// the ratio accurate: the error in it is the sampling jitter of two message
/// sends divided by that window, and a read only happens after a submission has
/// completed.
///
/// The result is mapped onto the host clock rather than left as scaled ticks:
/// `base` is a fixed pair, so two values read in different calls are still
/// comparable, where a bare `ticks × ratio` would shift as the ratio converged.
///
/// # Exact, because the correlation makes it exact
///
/// `elapsed × cpu_span / gpu_span` is a rational with integer ends, so there is
/// no float anywhere in it: the product is taken in `u128`, which holds a full
/// `u64` times a full `u64`, and the division rounds to nearest by adding half
/// the divisor first. Nothing here quantises the way an `f64` multiply would
/// once a free-running counter passes 2⁵³, which is the same argument
/// `crcbl_vk::conv::timestamp_nanos` and `crcbl_dx12::query::timestamp_nanos`
/// make for their own arithmetic.
///
/// # Zero is the answer when there is no clock
///
/// A device whose GPU timestamp does not move between the two readings — which
/// is what the Mac in CI answers, measured by `crate::adapter`'s counter probe —
/// has no rate to derive, and [`COUNTER_ERROR`] is a sample that was never
/// taken. Both report `0`, which is what the seam's degrading rule asks for: the
/// HUD shows blanks, the frame still renders.
pub(crate) fn timestamp_nanos(ticks: u64, base: Correlation, now: Correlation) -> u64 {
    if ticks == COUNTER_ERROR {
        return 0;
    }
    let gpu_span = now.gpu.saturating_sub(base.gpu);
    if gpu_span == 0 {
        return 0;
    }
    let cpu_span = now.cpu.saturating_sub(base.cpu);
    let elapsed = u128::from(ticks.saturating_sub(base.gpu));
    let divisor = u128::from(gpu_span);
    let nanos = (elapsed * u128::from(cpu_span) + divisor / 2) / divisor;
    u64::try_from(u128::from(base.cpu) + nanos).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A set of no queries is refused, and the two widths are the two Metal
    /// structures.
    ///
    /// Red if `PipelineStatistics` falls back to eight bytes, which is the
    /// width every other kind has and the one a resolve would overrun by
    /// [`STATISTIC_COUNTERS`] times.
    #[test]
    fn a_statistics_query_is_wider_than_a_timestamp_and_a_set_of_none_is_refused() {
        assert_eq!(result_bytes(QueryKind::Occlusion), size_of::<u64>() as u64);
        assert_eq!(result_bytes(QueryKind::Timestamp), size_of::<u64>() as u64);
        assert_eq!(
            result_bytes(QueryKind::PipelineStatistics),
            STATISTIC_COUNTERS * size_of::<u64>() as u64
        );
        assert!(
            result_bytes(QueryKind::PipelineStatistics) > result_bytes(QueryKind::Timestamp),
            "a statistics resolve writes more than one u64 per query, and every bound below \
             depends on knowing it"
        );

        assert_eq!(
            buffer_bytes(4).expect("four queries"),
            4 * size_of::<u64>() as u64
        );
        assert_eq!(buffer_bytes(1).expect("one query"), size_of::<u64>() as u64);

        let error = buffer_bytes(0).expect_err("no queries");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let error = check_count(0).expect_err("no samples");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");

        // Query `n` starts where the previous one ended, which is the whole of
        // what makes a set a flat array rather than a Metal object.
        assert_eq!(span_bytes(QueryKind::Timestamp, 0), 0);
        assert_eq!(
            span_bytes(QueryKind::Timestamp, 3),
            3 * result_bytes(QueryKind::Timestamp)
        );
        assert_eq!(
            span_bytes(QueryKind::PipelineStatistics, 3),
            3 * result_bytes(QueryKind::PipelineStatistics)
        );
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

    /// A resolve destination must hold what Metal is about to write into it,
    /// **at the width the kind resolves with**.
    ///
    /// Red if the check is dropped or done at one `u64` per query: without it a
    /// two-query resolve into an eight-byte buffer reaches the blit encoder,
    /// which bounds-checks nothing and raises — and a Metal raise aborts the
    /// process rather than failing the call.
    #[test]
    fn a_resolve_destination_must_hold_every_query_at_its_own_width() {
        check_destination(QueryKind::Occlusion, 0, 2, 16).expect("exactly two queries");
        check_destination(QueryKind::Timestamp, 8, 1, 16).expect("the second slot");
        check_destination(QueryKind::Timestamp, 16, 0, 16).expect("an empty resolve at the end");

        let error = check_destination(QueryKind::Timestamp, 8, 2, 16).expect_err("one too many");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(error.to_string().contains("24"), "{error}");

        // The whole point of the width: a destination that holds two timestamps
        // holds no statistics query at all.
        check_destination(QueryKind::Timestamp, 0, 2, 16).expect("two timestamps");
        let error =
            check_destination(QueryKind::PipelineStatistics, 0, 2, 16).expect_err("two statistics");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        check_destination(
            QueryKind::PipelineStatistics,
            0,
            2,
            2 * STATISTIC_COUNTERS * size_of::<u64>() as u64,
        )
        .expect("a destination sized for the real width");

        // Overflow is refused rather than wrapping into a range that fits.
        let error =
            check_destination(QueryKind::Timestamp, u64::MAX - 7, 2, u64::MAX).expect_err("wraps");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
    }

    /// A counter resolve's offset must be one Metal will accept.
    ///
    /// Red if the check is dropped: an unaligned `destinationOffset` is a rule
    /// `resolveCounters:…` states and does not diagnose.
    #[test]
    fn a_counter_resolve_offset_is_checked_against_the_alignment_this_device_promises() {
        check_resolve_alignment(0, 256).expect("the start of a buffer is always aligned");
        check_resolve_alignment(512, 256).expect("two strides in");

        let error = check_resolve_alignment(8, 256).expect_err("eight is not a multiple of 256");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        let text = error.to_string();
        assert!(text.contains('8') && text.contains("256"), "{text}");
    }

    /// The seam's two rules about a pass's timestamp pair, plus the kind check.
    ///
    /// Red if any of the three is dropped. Coincident indices measure nothing;
    /// an index past the end reaches a setter Metal does not bounds-check; and
    /// an occlusion set has no `sampleBuffer` for a pass to attach at all.
    #[test]
    fn a_passs_timestamp_pair_is_checked_against_the_set_it_names() {
        // Never looked at — this checks the two indices and the kind, and the
        // handle is resolved by the caller before it gets here.
        let set = crcbl_core::Handle::from_bits(1 << 32).expect("a non-zero generation");
        let good = PassTimestampWrites {
            set,
            beginning_of_pass: 0,
            end_of_pass: 1,
        };
        check_timestamp_pair("begin_render_pass", &good, QueryKind::Timestamp, 2)
            .expect("a legal pair");

        let coincident = PassTimestampWrites {
            end_of_pass: 0,
            ..good
        };
        let error = check_timestamp_pair("begin_render_pass", &coincident, QueryKind::Timestamp, 2)
            .expect_err("both ends in one query");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(
            error.to_string().contains("begin_render_pass"),
            "the message must name the verb: {error}"
        );

        let past_the_end = PassTimestampWrites {
            end_of_pass: 2,
            ..good
        };
        let error =
            check_timestamp_pair("begin_compute_pass", &past_the_end, QueryKind::Timestamp, 2)
                .expect_err("one past the end");
        assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        assert!(error.to_string().contains("begin_compute_pass"), "{error}");

        for kind in [QueryKind::Occlusion, QueryKind::PipelineStatistics] {
            let error = check_timestamp_pair("begin_render_pass", &good, kind, 2)
                .expect_err("the wrong kind of set");
            assert!(matches!(error, HalError::InvalidDescriptor(_)), "{error:?}");
        }
    }

    /// **A GPU tick is worth what the two correlations say it is worth.**
    ///
    /// Red if the scale is dropped, if the base is not subtracted, or if the
    /// division truncates: the Intel case below is `1000/12` nanoseconds per
    /// tick, where a truncating divide lands one nanosecond low.
    #[test]
    fn a_timestamp_is_placed_on_the_host_clock_by_two_correlations() {
        // Apple silicon: the GPU clock already counts nanoseconds, so the ratio
        // is one and a tick maps to itself plus the base offset.
        let base = Correlation {
            cpu: 1_000,
            gpu: 500,
        };
        let now = Correlation {
            cpu: 1_000_000_000 + 1_000,
            gpu: 1_000_000_000 + 500,
        };
        assert_eq!(timestamp_nanos(500, base, now), 1_000);
        assert_eq!(timestamp_nanos(1_500, base, now), 2_000);

        // Intel: 1000 host nanoseconds per 12 GPU ticks — 83.333… each — so 12
        // ticks past the base is 1000 nanoseconds past it, exactly.
        let now = Correlation {
            cpu: 1_000 + 1_000_000,
            gpu: 500 + 12_000,
        };
        assert_eq!(timestamp_nanos(500 + 12_000, base, now), 1_000 + 1_000_000);
        assert_eq!(timestamp_nanos(500 + 12, base, now), 1_000 + 1_000);
        // Rounds to nearest rather than truncating: 3 ticks is 250 ns exactly,
        // and 1 tick is 83.33…, which is 83 rather than 83-and-something-lost.
        assert_eq!(timestamp_nanos(500 + 3, base, now), 1_000 + 250);
        assert_eq!(timestamp_nanos(500 + 1, base, now), 1_000 + 83);
        // Half rounds up.
        let half = Correlation { cpu: 5, gpu: 2 };
        assert_eq!(timestamp_nanos(1, Correlation { cpu: 0, gpu: 0 }, half), 3);

        // A counter past the f64 mantissa still resolves single nanoseconds,
        // which is the whole reason the arithmetic is integer.
        let base = Correlation { cpu: 0, gpu: 0 };
        let now = Correlation {
            cpu: 1_000_000_000,
            gpu: 1_000_000_000,
        };
        let past_the_mantissa = (1u64 << 53) + 8;
        assert_eq!(
            timestamp_nanos(past_the_mantissa, base, now),
            past_the_mantissa
        );
        assert_eq!(
            timestamp_nanos(past_the_mantissa + 1, base, now),
            past_the_mantissa + 1,
            "consecutive ticks must still differ by one nanosecond"
        );
    }

    /// The two ways a timestamp has no answer both report zero rather than a
    /// number.
    ///
    /// Red if either is dropped. A device whose GPU clock does not move — CI's
    /// Apple Paravirtual device, measured — would divide by zero; and
    /// [`COUNTER_ERROR`] is `u64::MAX`, which scaled would be reported as some
    /// hundreds of years and read as a real timing.
    #[test]
    fn an_unsampled_query_and_an_inert_clock_both_read_back_zero() {
        let base = Correlation { cpu: 7, gpu: 11 };
        let moving = Correlation {
            cpu: 7 + 1_000_000,
            gpu: 11 + 1_000_000,
        };
        assert_eq!(timestamp_nanos(COUNTER_ERROR, base, moving), 0);

        // `cpu_delta=0 gpu_delta=0` across a real 53 ms of wall clock is what
        // the Mac in CI answered; see `crate::adapter`'s counter probe.
        assert_eq!(timestamp_nanos(11, base, base), 0);
        assert_eq!(timestamp_nanos(1_000_000, base, base), 0);

        // And a saturating conversion rather than a wrap, for a ratio that
        // would carry a real tick past the end of a u64.
        let exploding = Correlation {
            cpu: u64::MAX,
            gpu: 1,
        };
        assert_eq!(
            timestamp_nanos(u64::MAX - 1, Correlation { cpu: 0, gpu: 0 }, exploding),
            u64::MAX
        );
    }
}
