//! The face as a cluster hierarchy — `docs/plan/sample/14-quarry.md`'s
//! milestone 2, and `docs/plan/25-lod.md`'s subject.
//!
//! [`crate::scene`] describes the face as one flat mesh: every cluster is drawn
//! at full detail from every camera. That is the right shape for milestone 1
//! and the wrong one for the sample, whose whole premise is a face receding 180
//! metres so that near and far clusters want different levels *of the same
//! mesh*. This module builds the levels.
//!
//! # Where the normals come from, since the decimator carries none
//!
//! `crcbl_scene::simplify` is position-only and says so: a coarse level's
//! vertices are wherever the collapses put them and belong to no vertex of the
//! level below, so there is no attribute to interpolate.
//! [`crcbl::render::scene::Geometry::Dag`] is where the caller
//! answers for that, and this one recomputes each level's normals from **its
//! own triangles**, with `crate::face::normals_of` — the same function level 0
//! went through.
//!
//! The alternative was the dunes patch's: evaluate the analytic height field at
//! the coarse vertex and take its gradient. It was not taken, and the reason is
//! worth keeping. The gradient at a coarse vertex is the *fine* surface's
//! normal — every ripple the decimator just removed, shaded back on — which is
//! normal mapping arriving by accident, and it would make a level look unlike
//! its own silhouette. Recomputing keeps each level self-consistent: what is
//! shaded is what is drawn.

use std::borrow::Cow;

use crcbl::render::scene::{Capacities, Geometry, MeshDesc, PageDesc, ProbeGrid, SceneDesc};
use crcbl::scene::cluster_dag::{ClusterDagError, build_cluster_dag};
use crcbl::shaders::cluster_dag::ClusterDag;

use crate::face::{Face, normals_of};
use crate::scene::{HEADROOM, PAGE_EXTENT, ROCK, vertex_bytes};

/// The face's cluster DAG: level 0 is the face itself, and each level above it
/// is the one below grouped, simplified and resplit.
///
/// # Errors
///
/// [`ClusterDagError`] if the face is not a triangle list the clusterer accepts
/// — a partial triangle or an index outside the mesh. [`crate::face`] produces
/// neither, so this is a refusal about a caller's own arrays.
pub fn quarry_dag(face: &Face) -> Result<ClusterDag, ClusterDagError> {
    Ok(build_cluster_dag(&face.positions, &face.indices)?.cook())
}

/// The face as a levelled scene, on [`crate::scene::quarry_scene`]'s terms
/// exactly — one rock material as row 0, a one-texel page, computed pools —
/// with the flat mesh replaced by the DAG.
///
/// # Errors
///
/// [`ClusterDagError`], from [`quarry_dag`].
pub fn dag_scene(face: &Face) -> Result<SceneDesc<'static>, ClusterDagError> {
    let dag = quarry_dag(face)?;
    let levels: Vec<Cow<'static, [u8]>> = dag
        .levels
        .iter()
        .map(|level| {
            let normals = normals_of(&level.positions, &level.indices());
            Cow::Owned(vertex_bytes(&level.positions, &normals))
        })
        .collect();

    Ok(SceneDesc {
        capacities: dag_capacities(&dag),
        meshes: vec![MeshDesc {
            label: Cow::Borrowed("quarry face"),
            geometry: Geometry::Dag {
                // Every level's, because nothing here samples the page — see
                // `crate::scene::vertex_bytes`, which gives every vertex of
                // every level the same coordinate.
                uv_range: crate::scene::uv_range(),
                levels,
                dag,
            },
        }],
        materials: vec![ROCK],
        page: PageDesc::opaque_white(PAGE_EXTENT),
        probes: ProbeGrid::default(),
    })
}

/// Pools that hold every level, not just the finest.
///
/// **A DAG is resident all at once.** Selection picks which clusters are
/// *drawn* each frame; every level's vertices and indices live in the pool
/// throughout, so the reservation is the sum over levels and not the base
/// mesh's count. Reserving level 0's would be refused at `with_scene` the first
/// time a second level existed.
#[must_use]
pub fn dag_capacities(dag: &ClusterDag) -> Capacities {
    let vertices: usize = dag.levels.iter().map(|level| level.positions.len()).sum();
    let indices: usize = dag.levels.iter().map(|level| level.indices().len()).sum();
    let scale = |count: usize| -> u32 {
        // `as` saturates at the maximum for a float above it, which is what is
        // wanted: a DAG too large for a `u32` pool is refused by `with_scene`
        // naming the pool rather than wrapping to a small one that is accepted
        // and overruns.
        (count as f32 * HEADROOM).ceil() as u32
    };
    Capacities {
        vertices: scale(vertices),
        indices: scale(indices),
        // One entry per level: `Geometry::levels()` is what the mesh table is
        // counted in, and a DAG occupies one row per level rather than one row.
        meshes: dag.levels.len() as u32,
        instances: 1,
        materials: 1,
        lights: 1,
        probes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small enough to build quickly and large enough to coarsen: a face that
    /// clusters into one cluster produces a single level and would make every
    /// assertion here vacuous.
    const CELLS: u32 = 64;

    /// **The face coarsens.** The whole premise of the milestone: if
    /// `build_cluster_dag` stopped at level 0 there would be no hierarchy to
    /// select from, and every later assertion would be about a flat mesh
    /// wearing a DAG's type.
    #[test]
    fn the_face_has_more_than_one_level() {
        let dag = quarry_dag(&crate::face::quarry_face(CELLS)).expect("the face clusters");
        assert!(
            dag.levels.len() > 1,
            "the face produced {} level(s), so nothing was simplified",
            dag.levels.len()
        );
    }

    /// **Each level holds fewer clusters than the one below**, which is the
    /// condition `build_cluster_dag` stops on and therefore the shape of what it
    /// returns. Asserted because a hierarchy that stopped shrinking would still
    /// have levels and would buy nothing.
    #[test]
    fn every_level_is_coarser_than_the_one_below() {
        let dag = quarry_dag(&crate::face::quarry_face(CELLS)).expect("the face clusters");
        for pair in dag.levels.windows(2) {
            let (below, above) = (&pair[0], &pair[1]);
            assert!(
                above.clusters.clusters.len() < below.clusters.clusters.len(),
                "a level of {} cluster(s) sits above one of {}",
                above.clusters.clusters.len(),
                below.clusters.clusters.len()
            );
        }
    }

    /// **Level 0 is the face, untouched.** `build_cluster_dag` documents it and
    /// selection depends on it: the finest level is what a near cluster falls
    /// back to, so a base that had been simplified would put a floor under the
    /// detail nothing could see past.
    #[test]
    fn level_zero_is_the_face_itself() {
        let face = crate::face::quarry_face(CELLS);
        let dag = quarry_dag(&face).expect("the face clusters");
        assert_eq!(dag.levels[0].positions, face.positions);
    }

    /// **Every level carries a vertex for every position it has.** The array
    /// `Geometry::Dag` zips against the DAG, and the one `check_scene` measures
    /// — a short one leaves a level's clusters reading the level below's
    /// vertices, which draws a picture rather than failing.
    #[test]
    fn every_level_is_packed_to_its_own_vertex_count() {
        let face = crate::face::quarry_face(CELLS);
        let scene = dag_scene(&face).expect("the face clusters");
        let Geometry::Dag { levels, dag, .. } = &scene.meshes[0].geometry else {
            panic!("dag_scene describes a DAG");
        };
        assert_eq!(levels.len(), dag.levels.len());
        for (bytes, level) in levels.iter().zip(&dag.levels) {
            assert_eq!(
                bytes.len(),
                level.positions.len() * crcbl::shaders::mesh::VERTEX_STRIDE
            );
        }
    }

    /// **The pools hold every level, summed** — not the finest one with room to
    /// spare.
    ///
    /// The assertion is against the total on purpose. Written as "more than
    /// level 0" it passed while `dag_capacities` summed level 0 alone, because
    /// [`HEADROOM`] already puts the reservation above that count: the test was
    /// measuring the padding rather than the summation it claimed to guard.
    #[test]
    fn the_pools_reserve_every_level_and_not_just_the_finest() {
        let face = crate::face::quarry_face(CELLS);
        let dag = quarry_dag(&face).expect("the face clusters");
        let capacities = dag_capacities(&dag);
        let vertices: usize = dag.levels.iter().map(|level| level.positions.len()).sum();
        let indices: usize = dag.levels.iter().map(|level| level.indices().len()).sum();
        assert!(
            capacities.vertices as usize >= vertices,
            "the vertex pool reserves {} and the DAG's levels hold {vertices} between them",
            capacities.vertices,
        );
        assert!(
            capacities.indices as usize >= indices,
            "the index pool reserves {} and the DAG's levels hold {indices} between them",
            capacities.indices,
        );
        assert_eq!(capacities.meshes as usize, dag.levels.len());
    }
}
