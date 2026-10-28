//! Pipeline-statistics queries: which counters a pool enables, how the values
//! Vulkan writes for one query land in the seam's [`PipelineStatistics`], and
//! why one seam query is two Vulkan pools.
//!
//! A statistics query is one `u64` per enabled counter, written in the order
//! of the counters' bits, lowest first (the specification's "Pipeline
//! Statistics Queries"). The pool's flags are therefore the whole layout, and
//! [`decode`] reads it from them rather than from a second list that could
//! disagree.
//!
//! # Two pools, because a mesh draw forbids the vertex counters
//!
//! A mesh draw may not run while a query counting input assembly or the vertex
//! stage is active (`VUID-vkCmdDrawMeshTasksEXT-pipelineStatistics-07076`),
//! and a pass on the mesh path still holds vertex draws (a sky, a full-screen
//! triangle). So a statistics set is a [`raster_flags`] pool and, on a device
//! with mesh shading, a [`mesh_flags`] pool, and the encoder counts each pass
//! in at most two segments: the raster pool from the pass's first draw, then
//! the mesh pool from its first mesh draw to its end. The mesh pool counts only
//! stages valid under both kinds of draw, so it never has to hand back.
//! [`merge`] sums the segments into the pass's statistics.

use ash::vk;
use crcbl_hal::PipelineStatistics;

type Flag = vk::QueryPipelineStatisticFlags;

/// Where one counter's value goes.
type Slot = fn(&mut PipelineStatistics) -> &mut Option<u64>;

/// Every counter this backend can enable, **in ascending bit order**, with the
/// field it fills. Geometry and tessellation counters are absent because the
/// seam has neither stage.
const COUNTERS: [(Flag, Slot); 9] = [
    (Flag::INPUT_ASSEMBLY_VERTICES, |s| &mut s.input_vertices),
    (Flag::INPUT_ASSEMBLY_PRIMITIVES, |s| &mut s.input_primitives),
    (Flag::VERTEX_SHADER_INVOCATIONS, |s| {
        &mut s.vertex_invocations
    }),
    (Flag::CLIPPING_INVOCATIONS, |s| &mut s.clipper_invocations),
    (Flag::CLIPPING_PRIMITIVES, |s| &mut s.clipper_primitives),
    (Flag::FRAGMENT_SHADER_INVOCATIONS, |s| {
        &mut s.fragment_invocations
    }),
    (Flag::COMPUTE_SHADER_INVOCATIONS, |s| {
        &mut s.compute_invocations
    }),
    (Flag::TASK_SHADER_INVOCATIONS_EXT, |s| {
        &mut s.task_invocations
    }),
    (Flag::MESH_SHADER_INVOCATIONS_EXT, |s| {
        &mut s.mesh_invocations
    }),
];

/// The counters valid under any draw. Clipping is not among them: the
/// validation layer rejects a mesh draw under a query counting it, beside the
/// input-assembly and vertex counters the VUID names.
const SHARED: Flag = Flag::from_raw(
    Flag::FRAGMENT_SHADER_INVOCATIONS.as_raw() | Flag::COMPUTE_SHADER_INVOCATIONS.as_raw(),
);

/// The raster pool's counters: input assembly, the vertex stage and the
/// clipper beside the shared ones.
pub(crate) const fn raster_flags() -> Flag {
    Flag::from_raw(
        SHARED.as_raw()
            | Flag::INPUT_ASSEMBLY_VERTICES.as_raw()
            | Flag::INPUT_ASSEMBLY_PRIMITIVES.as_raw()
            | Flag::VERTEX_SHADER_INVOCATIONS.as_raw()
            | Flag::CLIPPING_INVOCATIONS.as_raw()
            | Flag::CLIPPING_PRIMITIVES.as_raw(),
    )
}

/// The mesh pool's counters: the shared ones, and the task and mesh stages
/// when the device was opened with `meshShaderQueries`, without which naming
/// them is invalid.
pub(crate) const fn mesh_flags(mesh_queries: bool) -> Flag {
    if mesh_queries {
        Flag::from_raw(
            SHARED.as_raw()
                | Flag::TASK_SHADER_INVOCATIONS_EXT.as_raw()
                | Flag::MESH_SHADER_INVOCATIONS_EXT.as_raw(),
        )
    } else {
        SHARED
    }
}

/// How many `u64`s one query of a pool with `flags` holds.
pub(crate) fn width(flags: Flag) -> usize {
    flags.as_raw().count_ones() as usize
}

/// One query's values, `width(flags)` of them, as the seam's statistics.
/// Counters the pool did not enable stay `None`; Vulkan has no counter for
/// [`PipelineStatistics::mesh_primitives`], so it is always `None` here.
pub(crate) fn decode(flags: Flag, values: &[u64]) -> PipelineStatistics {
    let mut statistics = PipelineStatistics::default();
    let mut values = values.iter().copied();
    for (flag, slot) in COUNTERS {
        if flags.contains(flag) {
            *slot(&mut statistics) = values.next();
        }
    }
    statistics
}

/// One pass's statistics from its two segments, each `None` where that pool's
/// query was never written.
///
/// A segment that did not happen counted nothing: so a pass with a raster
/// segment alone reports the mesh pool's counters as zero, which is what a
/// pass with no mesh draw did. A pass with a mesh segment reports input
/// assembly, the vertex stage and the clipper as `None`, because a draw after
/// its first mesh draw was counted only in the shared stages and nothing here
/// says whether one happened: a partial count would read as the whole.
pub(crate) fn merge(
    raster: Option<PipelineStatistics>,
    mesh: Option<PipelineStatistics>,
    mesh_pool: Option<Flag>,
) -> PipelineStatistics {
    let raster = match mesh {
        Some(_) => raster.map(|counted| decode_shared(&counted)),
        None => raster,
    };
    let mesh = match (raster, mesh, mesh_pool) {
        (Some(_), None, Some(flags)) => Some(decode(flags, &vec![0; width(flags)])),
        _ => mesh,
    };
    let [raster, mesh] = [raster, mesh].map(Option::unwrap_or_default);
    let mut statistics = PipelineStatistics::default();
    for (_, slot) in COUNTERS {
        let (mut raster, mut mesh) = (raster, mesh);
        *slot(&mut statistics) = match (*slot(&mut raster), *slot(&mut mesh)) {
            (Some(a), Some(b)) => Some(a + b),
            (a, b) => a.or(b),
        };
    }
    statistics
}

/// `statistics` with only the [`SHARED`] counters kept.
fn decode_shared(statistics: &PipelineStatistics) -> PipelineStatistics {
    let mut statistics = *statistics;
    let mut kept = PipelineStatistics::default();
    for (flag, slot) in COUNTERS {
        if SHARED.contains(flag) {
            *slot(&mut kept) = *slot(&mut statistics);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Red if [`COUNTERS`] is out of bit order, which would hand every counter
    /// after the slip its neighbour's value.
    #[test]
    fn the_counters_are_listed_in_the_order_vulkan_writes_them() {
        let bits: Vec<u32> = COUNTERS.iter().map(|(flag, _)| flag.as_raw()).collect();
        let mut sorted = bits.clone();
        sorted.sort_unstable();
        assert_eq!(bits, sorted);
    }

    /// Each value reaches its own field: distinct values, so a slot pointing at
    /// the wrong field shows up as a swapped pair.
    #[test]
    fn each_value_lands_in_its_own_field() {
        let flags = raster_flags() | mesh_flags(true);
        let values: Vec<u64> = (1..=width(flags) as u64).collect();
        assert_eq!(
            decode(flags, &values),
            PipelineStatistics {
                input_vertices: Some(1),
                input_primitives: Some(2),
                vertex_invocations: Some(3),
                clipper_invocations: Some(4),
                clipper_primitives: Some(5),
                fragment_invocations: Some(6),
                compute_invocations: Some(7),
                task_invocations: Some(8),
                mesh_invocations: Some(9),
                mesh_primitives: None,
            }
        );
    }

    /// The mesh pool never names a counter a mesh draw forbids, with or
    /// without the mesh stages.
    #[test]
    fn the_mesh_pool_counts_nothing_a_mesh_draw_forbids() {
        let forbidden = Flag::INPUT_ASSEMBLY_VERTICES
            | Flag::INPUT_ASSEMBLY_PRIMITIVES
            | Flag::VERTEX_SHADER_INVOCATIONS
            | Flag::CLIPPING_INVOCATIONS
            | Flag::CLIPPING_PRIMITIVES;
        for mesh_queries in [false, true] {
            assert!(!mesh_flags(mesh_queries).intersects(forbidden));
        }
        assert!(raster_flags().contains(forbidden));
    }

    /// Without mesh queries the mesh fields are `None`, and nothing before
    /// them shifts.
    #[test]
    fn a_pool_without_mesh_queries_reports_no_mesh_counts() {
        let flags = mesh_flags(false);
        let values: Vec<u64> = (1..=width(flags) as u64).collect();
        let statistics = decode(flags, &values);
        assert_eq!(statistics.compute_invocations, Some(2));
        assert_eq!(statistics.task_invocations, None);
        assert_eq!(statistics.mesh_invocations, None);
    }

    fn segment(flags: Flag, value: u64) -> PipelineStatistics {
        decode(flags, &vec![value; width(flags)])
    }

    /// Both segments: the shared stages add, the mesh stages come from the
    /// mesh pool, and the raster pool's own stages are not reported, since a
    /// vertex draw after the first mesh draw is missing from them.
    #[test]
    fn two_segments_sum_their_shared_stages() {
        let merged = merge(
            Some(segment(raster_flags(), 2)),
            Some(segment(mesh_flags(true), 5)),
            Some(mesh_flags(true)),
        );
        assert_eq!(merged.vertex_invocations, None);
        assert_eq!(merged.clipper_primitives, None);
        assert_eq!(merged.fragment_invocations, Some(7));
        assert_eq!(merged.mesh_invocations, Some(5));
    }

    /// A raster segment alone: the mesh stages did nothing, and say so as
    /// zero where the device counts them and `None` where it cannot.
    #[test]
    fn a_raster_segment_alone_reports_zero_mesh_work() {
        let raster = Some(segment(raster_flags(), 2));
        let counted = merge(raster, None, Some(mesh_flags(true)));
        assert_eq!(counted.mesh_invocations, Some(0));
        assert_eq!(counted.fragment_invocations, Some(2));
        let uncounted = merge(raster, None, Some(mesh_flags(false)));
        assert_eq!(uncounted.mesh_invocations, None);
        assert_eq!(merge(raster, None, None).task_invocations, None);
    }

    /// A mesh segment alone cannot speak for the vertex stage or the clipper.
    #[test]
    fn a_mesh_segment_alone_reports_no_vertex_counts() {
        let merged = merge(
            None,
            Some(segment(mesh_flags(true), 3)),
            Some(mesh_flags(true)),
        );
        assert_eq!(merged.vertex_invocations, None);
        assert_eq!(merged.clipper_primitives, None);
        assert_eq!(merged.mesh_invocations, Some(3));
    }

    /// Neither segment written: nothing to speak for.
    #[test]
    fn no_segment_is_no_statistics() {
        assert_eq!(
            merge(None, None, Some(mesh_flags(true))),
            PipelineStatistics::default()
        );
    }
}
