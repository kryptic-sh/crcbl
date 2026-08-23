//! LOD chains: one mesh becomes a chain of decimated, clustered levels.
//!
//! `docs/plan/25-lod.md`'s "LOD chains" section asks for "a chain of N levels
//! (LOD0 = full quality … LODn)", each carrying the error that producing it
//! cost, auto-generated "by decimating the base mesh to that level's target
//! ratio (default chain ~50/25/12/6%, per-asset overridable)". This module is
//! the composition of the two builders that already exist — [`simplify`]
//! decimates, [`build_meshlets`] clusters — into that chain.
//!
//! # Every level is decimated from LOD0, not from the level above
//!
//! The quadric a collapse is costed against is the sum of the planes of the
//! faces folded into it *since that run started* (§5 of Garland–Heckbert; see
//! [`mod@crate::simplify`] for the transcription). So the number
//! [`Simplified::max_error`](crate::Simplified::max_error) reports is
//! measured against the mesh that run was given and against nothing else.
//!
//! That decides the question. Runtime selection projects a level's error into
//! screen space to answer *may this level stand in for the full-quality mesh*,
//! so every level's error has to be measured against the same thing — LOD0 —
//! or the numbers are not on one scale and comparing them to each other, or to
//! one pixel budget, is meaningless. Decimating LOD2 out of LOD1 would report
//! LOD2's error against LOD1 and would understate it against LOD0 by whatever
//! LOD1 already cost. Cascading is the cheaper build and it is the one that
//! cannot honestly fill in the `error` column, so each level here is a fresh
//! decimation of the base arrays to its own target. The cost is that the work
//! is redone per level rather than continued; this is a bake step, not a frame
//! path.
//!
//! Deciding it that way also makes the chain's errors non-decreasing by
//! construction. [`simplify`] reads its target only in its loop condition —
//! nothing about which collapse happens next depends on it — so a run to a
//! smaller target performs exactly the collapses of a run to a larger one and
//! then keeps going, and the error it reports is a running maximum over that
//! sequence. A longer sequence cannot report less — which is what
//! `a_coarser_target_gives_a_different_mesh_and_at_least_as_much_error` checks
//! over there, for one pair of targets. [`build_lod_chain`] asserts it for
//! every adjacent pair of its own levels rather than resting on the argument.
//!
//! # A chain, not a DAG — so this supports per-instance selection only
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.5 and topic 25's "Runtime
//! selection" both say detail is chosen **per cluster** on the `MeshShader`
//! path and per instance elsewhere. A chain of independently clustered levels
//! cannot do the per-cluster half, and this module does not pretend otherwise.
//!
//! Each level is clustered on its own: [`build_meshlets`] grows clusters over
//! that level's own triangles, so two levels' cluster boundaries have no
//! relationship to each other, and the decimation between them moved and
//! removed vertices wherever the collapses fell — including along whatever boundary the coarser
//! level ends up drawn on. Rendering one cluster from LOD1 beside its
//! neighbour from LOD2 therefore puts two differently decimated versions of one
//! shared edge next to each other, which is a crack.
//!
//! The published answer is a DAG rather than a chain, in the shape Nanite uses
//! (Brian Karis, *Nanite: A Deep Dive*, SIGGRAPH 2021 Advances in Real-Time
//! Rendering course): group neighbouring clusters, lock each group's shared
//! boundary while simplifying the group, re-split the simplified group into
//! fresh clusters, and repeat — so any two clusters that can be drawn together
//! still agree on the edge between them, whatever cut across the DAG selection
//! makes. That is a different builder rather than a parameter on this one, and
//! it is not in this slice. Until it exists, these levels are selectable per
//! instance and not per cluster.
//!
//! # This is the chain builder and nothing else
//!
//! [`build_lod_chain`] takes host arrays and returns host arrays. There is no
//! runtime selection in the cull or amplification stage, no screen-space error
//! projection, no hysteresis, no shadow bias, no hand-authored level
//! precedence and no `MSFT_lod` parsing, no sidecar ratio overrides, no
//! `crcbl lod` CLI, no bake cache and no GPU upload; nothing in the engine
//! calls this yet. Every one of those is a later slice of topic 25. Read this
//! as the input to that work rather than as a working LOD system.

use crate::meshlet::{MeshletBuild, MeshletError, build_meshlets};
use crate::simplify::{SimplifyError, simplify};

/// The plan's default chain: each level half the triangles of the one above.
///
/// `docs/plan/25-lod.md` writes it as "~50/25/12/6%", which is the rounded
/// form of the halvings this holds. A caller with an asset that wants a
/// different curve passes its own ratios to [`build_lod_chain`] — the sidecar
/// meta the plan overrides them from is a later slice.
pub const DEFAULT_LOD_RATIOS: [f32; 4] = [0.5, 0.25, 0.125, 0.0625];

/// Why a LOD chain could not be built.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum LodError {
    /// A ratio is not inside `(0, 1)`, or is not below the ratio before it.
    ///
    /// A chain's levels get coarser, so its ratios strictly decrease, and both
    /// halves of that are refused rather than reordered: a caller who wrote
    /// them out of order meant something this function cannot guess, and a
    /// chain whose targets do not decrease is one whose errors are no longer
    /// non-decreasing either.
    ///
    /// `level` counts from 1, because LOD0 is the base mesh and has no ratio.
    #[error("LOD{level}'s ratio {ratio} is not inside (0, 1) and below the level above it")]
    Ratio {
        /// Which level's ratio was refused, counting LOD0 as the base.
        level: usize,
        /// The refused ratio.
        ratio: f32,
    },

    /// A level could not be decimated.
    ///
    /// Both [`SimplifyError`] variants are properties of the base arrays,
    /// which every level is decimated from, so this says nothing about which
    /// level was being built when it fired.
    #[error(transparent)]
    Simplify(#[from] SimplifyError),

    /// A level could not be clustered.
    #[error(transparent)]
    Cluster(#[from] MeshletError),
}

/// One level of a LOD chain: a mesh, its clusters, and its error.
///
/// The plan's LOD table is `[(base_vertex, base_index, count, error); N]` —
/// pool ranges, which need pools. This is the host-memory form the bake
/// produces before any upload exists: the geometry itself rather than a range
/// naming it, plus the [`MeshletBuild`] over that geometry, plus the same
/// `error`.
#[derive(Clone, Debug, PartialEq)]
pub struct LodLevel {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    clusters: MeshletBuild,
    error: f32,
}

impl LodLevel {
    /// This level's vertices.
    ///
    /// LOD0's are the base mesh's verbatim. A decimated level's are
    /// [`Simplified::positions`](crate::Simplified::positions): the
    /// survivors, renumbered densely, so a coarser level has both fewer
    /// triangles and fewer vertices than the one above it.
    #[inline]
    #[must_use]
    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    /// This level's triangle list, indexing [`positions`](Self::positions).
    #[inline]
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// This level's clusters, built over this level's own geometry.
    ///
    /// Every cluster's corners decode through
    /// [`MeshletBuild::vertices`](crate::MeshletBuild::vertices) to indices
    /// into [`positions`](Self::positions) — never into another level's.
    #[inline]
    #[must_use]
    pub fn clusters(&self) -> &MeshletBuild {
        &self.clusters
    }

    /// How far this level's surface departs from LOD0's, in the mesh's own
    /// units of length.
    ///
    /// [`Simplified::max_error`](crate::Simplified::max_error) for the
    /// decimation that produced it, and zero for LOD0, which is the base mesh
    /// untouched. Every level in one chain is
    /// measured against LOD0 and the numbers are therefore comparable to each
    /// other — that is the whole reason each level is decimated from the base;
    /// see this module's own docs. What the number does *not* claim is set out
    /// on [`Simplified::max_error`](crate::Simplified::max_error): it is a
    /// quadric error and has not been shown to dominate a sampled Hausdorff
    /// distance.
    #[inline]
    #[must_use]
    pub fn error(&self) -> f32 {
        self.error
    }
}

/// Build a LOD chain: LOD0 plus one decimated level per ratio.
///
/// `positions` and `indices` are a triangle list exactly as
/// [`GltfPrimitive`](crate::GltfPrimitive) holds them, and become LOD0
/// verbatim. Each ratio is a fraction of LOD0's *triangle* count, rounded, and
/// must be inside `(0, 1)` and below the one before it; [`DEFAULT_LOD_RATIOS`]
/// is the plan's chain. The result has `ratios.len() + 1` levels, so an empty
/// slice asks for LOD0 alone and gets exactly that.
///
/// Every level is clustered, LOD0 included, and every level is decimated from
/// the base rather than from the level above — see this module's docs for why
/// that is what makes the errors comparable.
///
/// A level can come out *above* its target: [`simplify`] stops early when no
/// remaining collapse is allowed, which a mesh whose every edge touches a
/// border does immediately. The level is still built, still clustered and
/// still honest about its error; a caller that cares compares
/// `indices().len() / 3` against the ratio it asked for. Two ratios that round
/// to the same target, or a target a stall lands on twice, give two identical
/// levels rather than an error.
///
/// The result is deterministic: the same arrays and ratios produce an
/// identical chain, because both halves it composes are.
///
/// # Errors
///
/// [`LodError::Ratio`] when a ratio is out of range or not below its
/// predecessor — checked for every ratio before any decimation happens, so a
/// bad last ratio costs nothing. [`LodError::Simplify`] and
/// [`LodError::Cluster`] carry the two builders' own refusals of the input
/// arrays; both are checked against the base mesh before any level is built.
///
/// # Panics
///
/// If a level's error came out below the previous level's, which would break
/// the ordering runtime selection relies on. Nothing input-dependent reaches
/// it: the property follows from [`simplify`] reading its target only to
/// decide when to stop, so the assertion is a check on that staying true
/// rather than a case a caller can provoke.
pub fn build_lod_chain(
    positions: &[[f32; 3]],
    indices: &[u32],
    ratios: &[f32],
) -> Result<Vec<LodLevel>, LodError> {
    let mut coarser_than = 1.0f32;
    for (level, &ratio) in ratios.iter().enumerate() {
        if !(ratio > 0.0 && ratio < coarser_than) {
            return Err(LodError::Ratio {
                level: level + 1,
                ratio,
            });
        }
        coarser_than = ratio;
    }

    let base_triangles = indices.len() / 3;
    let mut chain = vec![LodLevel {
        positions: positions.to_vec(),
        indices: indices.to_vec(),
        clusters: build_meshlets(positions, indices)?,
        error: 0.0,
    }];

    for (level, &ratio) in ratios.iter().enumerate() {
        let decimated = simplify(positions, indices, target_triangles(base_triangles, ratio))?;
        let error = decimated.max_error();
        let above = chain[level].error();
        assert!(
            error >= above,
            "LOD{} reports error {error}, below LOD{level}'s {above}",
            level + 1
        );
        chain.push(LodLevel {
            clusters: build_meshlets(decimated.positions(), decimated.indices())?,
            positions: decimated.positions().to_vec(),
            indices: decimated.indices().to_vec(),
            error,
        });
    }

    Ok(chain)
}

/// How many triangles a level at `ratio` asks [`simplify`] for.
///
/// Rounded rather than truncated, so a ratio names the nearest achievable
/// level rather than always the coarser one, and floored at a single triangle:
/// a target of zero asks the decimator for a mesh with no faces, which is not
/// a level. A mesh with no triangles keeps its zero — there is nothing to
/// floor it to.
fn target_triangles(base_triangles: usize, ratio: f32) -> usize {
    let scaled = (base_triangles as f64 * f64::from(ratio)).round() as usize;
    if base_triangles == 0 {
        0
    } else {
        scaled.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meshlet::tests::{decoded, sorted, triangles_of};
    use crate::simplify::tests::{height_field, spikes, torus};

    /// Every level's triangle count, LOD0 first.
    fn triangle_counts(chain: &[LodLevel]) -> Vec<usize> {
        chain
            .iter()
            .map(|level| level.indices().len() / 3)
            .collect()
    }

    /// Asserts the chain is not the trivial one — N copies of the base mesh —
    /// which every other invariant here is satisfied by.
    fn assert_levels_got_coarser(chain: &[LodLevel]) {
        let counts = triangle_counts(chain);
        assert!(counts.len() > 1, "a chain of one level proves nothing");
        for pair in counts.windows(2) {
            assert!(
                pair[1] < pair[0],
                "level counts {counts:?} are not strictly decreasing"
            );
        }
    }

    /// A torus rather than a plane: a curved closed surface cannot be
    /// decimated for free, so its levels have errors to compare. A flat mesh
    /// reports zero at every level and satisfies "non-decreasing" without ever
    /// exercising it.
    fn torus_chain() -> Vec<LodLevel> {
        let (positions, indices) = torus(16, 12);
        build_lod_chain(&positions, &indices, &DEFAULT_LOD_RATIOS).unwrap()
    }

    #[test]
    fn a_chain_has_lod0_plus_one_level_per_ratio_at_its_target() {
        let (positions, indices) = torus(16, 12);
        let base = indices.len() / 3;

        let chain = build_lod_chain(&positions, &indices, &DEFAULT_LOD_RATIOS).unwrap();

        assert_eq!(chain.len(), DEFAULT_LOD_RATIOS.len() + 1);
        assert_eq!(chain[0].positions(), positions, "LOD0 is the base verbatim");
        assert_eq!(chain[0].indices(), indices);
        assert_eq!(chain[0].error(), 0.0);
        let targets: Vec<usize> = DEFAULT_LOD_RATIOS
            .iter()
            .map(|&ratio| target_triangles(base, ratio))
            .collect();
        assert_eq!(
            targets,
            [192, 96, 48, 24],
            "384 triangles halved four times"
        );
        assert_eq!(triangle_counts(&chain)[1..], targets);
        assert_levels_got_coarser(&chain);
    }

    #[test]
    fn the_ratios_are_a_parameter_rather_than_the_default_chain() {
        let (positions, indices) = torus(16, 12);

        let chain = build_lod_chain(&positions, &indices, &[0.75, 0.6]).unwrap();

        assert_eq!(
            triangle_counts(&chain),
            [384, 288, 230],
            "three quarters and three fifths of 384, rounded"
        );
        assert_levels_got_coarser(&chain);
    }

    /// The invariant runtime selection rests on: it picks the coarsest level
    /// under a pixel budget, so a level whose error dipped below its
    /// predecessor's would let selection skip the one in between.
    #[test]
    fn the_error_never_decreases_up_the_chain() {
        let chain = torus_chain();

        assert_levels_got_coarser(&chain);
        let errors: Vec<f32> = chain.iter().map(LodLevel::error).collect();
        assert!(
            errors[1] > 0.0,
            "a curved surface cannot be decimated for free, so {errors:?} has \
             to have something to compare"
        );
        assert!(
            errors[errors.len() - 1] > errors[1],
            "the coarsest level of {errors:?} has to cost more than the finest, \
             or 'non-decreasing' is vacuous here"
        );
        for pair in errors.windows(2) {
            assert!(pair[1] >= pair[0], "errors {errors:?} dip");
        }
        assert!(errors.iter().all(|error| error.is_finite()));
    }

    /// What "the error is measured against LOD0" comes down to, since the
    /// ordering above holds either way — a chain cascaded out of each previous
    /// level passes every other test in this module. Nothing but the
    /// provenance of the arrays fed to [`simplify`] decides it, so that is
    /// what this pins, and the second half is what says the two provenances
    /// are distinguishable on this fixture at all.
    #[test]
    fn every_level_is_a_fresh_decimation_of_the_base() {
        let (positions, indices) = torus(16, 12);
        let base = indices.len() / 3;

        let chain = build_lod_chain(&positions, &indices, &DEFAULT_LOD_RATIOS).unwrap();

        for (level, &ratio) in DEFAULT_LOD_RATIOS.iter().enumerate() {
            let alone = simplify(&positions, &indices, target_triangles(base, ratio)).unwrap();
            let lod = &chain[level + 1];
            let name = level + 1;
            assert_eq!(lod.indices(), alone.indices(), "LOD{name}");
            assert_eq!(lod.positions(), alone.positions(), "LOD{name}");
            assert_eq!(lod.error(), alone.max_error(), "LOD{name}");
        }

        let cascaded = simplify(
            chain[1].positions(),
            chain[1].indices(),
            target_triangles(base, DEFAULT_LOD_RATIOS[1]),
        )
        .unwrap();
        assert_eq!(
            cascaded.indices().len(),
            chain[2].indices().len(),
            "the same target, so only the geometry can differ"
        );
        assert_ne!(
            cascaded.indices(),
            chain[2].indices(),
            "cascading has to reach a different mesh here, or the equalities \
             above pin nothing"
        );
        assert!(
            cascaded.max_error() < chain[2].error(),
            "cascading reports {} against LOD1, which understates the {} this \
             level departs from LOD0 by",
            cascaded.max_error(),
            chain[2].error()
        );
    }

    /// The documented way a level misses its target: [`simplify`] locks the
    /// vertices on a border, so an open patch stops decimating once its outer
    /// ring is most of what is left. The level is still built and the chain is
    /// still ordered — it is the count that gives, not the invariant.
    #[test]
    fn a_level_that_stalls_above_its_target_is_still_a_level() {
        let (positions, indices) = height_field(24, spikes);
        let base = indices.len() / 3;

        let chain = build_lod_chain(&positions, &indices, &DEFAULT_LOD_RATIOS).unwrap();

        let counts = triangle_counts(&chain);
        let targets: Vec<usize> = DEFAULT_LOD_RATIOS
            .iter()
            .map(|&ratio| target_triangles(base, ratio))
            .collect();
        assert_eq!(counts, [1152, 576, 288, 144, 100]);
        assert_eq!(targets, [576, 288, 144, 72]);
        assert!(
            counts[4] > targets[3],
            "the fixture has to overshoot for this test to say anything"
        );
        // Where the coarsest level stops is the locked border and then
        // `Decimator::keeps_its_original_facing`: the outer ring of a 24 x 24
        // patch is 96 vertices and can never go, three interior vertices
        // survive because the collapses that would remove them are refused by
        // that check, and a triangulated polygon with `n` boundary and `k`
        // interior vertices has `2k + n - 2` faces. Without the check the same
        // chain bottoms out at one interior vertex.
        let border = 96;
        let interior = chain[4].positions().len() - border;
        assert_eq!(interior, 3);
        assert_eq!(counts[4], 2 * interior + border - 2);
        assert_levels_got_coarser(&chain);
        for pair in chain.windows(2) {
            assert!(pair[1].error() >= pair[0].error());
        }
        assert_eq!(
            sorted(decoded(chain[4].clusters())),
            sorted(triangles_of(chain[4].indices()))
        );
    }

    #[test]
    fn the_same_mesh_twice_gives_an_identical_chain() {
        let (positions, indices) = torus(16, 12);

        let first = build_lod_chain(&positions, &indices, &DEFAULT_LOD_RATIOS).unwrap();
        let second = build_lod_chain(&positions, &indices, &DEFAULT_LOD_RATIOS).unwrap();

        assert_levels_got_coarser(&first);
        assert_eq!(first, second);
    }

    /// [`build_meshlets`]' own decode invariant, applied per level: a level's
    /// corners have to name that level's own vertices, not the base mesh's.
    #[test]
    fn every_levels_clusters_decode_to_that_levels_own_triangles() {
        let chain = torus_chain();

        assert_levels_got_coarser(&chain);
        let cluster_counts: Vec<usize> = chain
            .iter()
            .map(|level| level.clusters().clusters().len())
            .collect();
        assert!(
            cluster_counts[0] > cluster_counts[cluster_counts.len() - 1],
            "clusters {cluster_counts:?} would not shrink if every level were \
             clustered from the same geometry"
        );
        for (level, lod) in chain.iter().enumerate() {
            assert!(
                !lod.clusters().clusters().is_empty(),
                "LOD{level} was not clustered at all"
            );
            assert_eq!(
                sorted(decoded(lod.clusters())),
                sorted(triangles_of(lod.indices())),
                "LOD{level}'s clusters do not decode to its own triangles"
            );
        }
    }

    #[test]
    fn ratios_that_do_not_decrease_into_the_unit_interval_are_refused() {
        let (positions, indices) = torus(8, 6);

        for (ratios, level, ratio) in [
            (vec![0.5, 0.5], 2, 0.5),
            (vec![0.5, 0.6], 2, 0.6),
            (vec![1.0], 1, 1.0),
            (vec![0.0], 1, 0.0),
            (vec![-0.5], 1, -0.5),
            (vec![f32::NAN], 1, f32::NAN),
        ] {
            let refused = build_lod_chain(&positions, &indices, &ratios).unwrap_err();

            match refused {
                LodError::Ratio {
                    level: refused_level,
                    ratio: refused_ratio,
                } => {
                    assert_eq!(refused_level, level, "{ratios:?}");
                    assert_eq!(refused_ratio.to_bits(), ratio.to_bits(), "{ratios:?}");
                }
                other => panic!("{ratios:?} was refused as {other:?}"),
            }
        }
    }

    #[test]
    fn no_ratios_gives_a_chain_of_lod0_alone() {
        let (positions, indices) = torus(8, 6);

        let chain = build_lod_chain(&positions, &indices, &[]).unwrap();

        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].indices(), indices);
        assert_eq!(
            sorted(decoded(chain[0].clusters())),
            sorted(triangles_of(&indices))
        );
    }

    #[test]
    fn a_mesh_with_no_triangles_gives_a_chain_of_empty_levels() {
        let chain = build_lod_chain(&[], &[], &DEFAULT_LOD_RATIOS).unwrap();

        assert_eq!(chain.len(), DEFAULT_LOD_RATIOS.len() + 1);
        for (level, lod) in chain.iter().enumerate() {
            assert!(lod.indices().is_empty(), "LOD{level} invented triangles");
            assert!(lod.clusters().clusters().is_empty());
            assert_eq!(lod.error(), 0.0);
        }
    }

    #[test]
    fn an_index_buffer_that_is_not_whole_triangles_is_refused() {
        let refused = build_lod_chain(&[[0.0; 3]], &[0, 0], &DEFAULT_LOD_RATIOS).unwrap_err();

        assert_eq!(
            refused,
            LodError::Cluster(MeshletError::PartialTriangle { count: 2 }),
            "LOD0's clustering is the first thing to read the arrays"
        );
    }

    #[test]
    fn an_index_outside_the_mesh_is_refused() {
        let refused = build_lod_chain(&[[0.0; 3]], &[0, 0, 7], &DEFAULT_LOD_RATIOS).unwrap_err();

        assert_eq!(
            refused,
            LodError::Cluster(MeshletError::IndexOutOfRange {
                index: 7,
                vertices: 1,
            })
        );
    }
}
