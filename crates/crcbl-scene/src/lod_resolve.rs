//! Which level came from where: hand-authored first, generated as fallback.
//!
//! `docs/plan/25-lod.md`'s "Hand-authored levels keep their precedence" states
//! the rule this module is, and states it as locked. Import resolves each level
//! in order:
//!
//! 1. **A hand-authored level exists** — the file's own geometry, reached
//!    through the `name_LOD1` node-naming convention or the `MSFT_lod`
//!    extension — and is used verbatim. It always wins.
//! 2. **A level is missing** — it is generated, by [`build_cluster_dag`].
//!
//! [`resolve_lod`] answers that for one node, and [`MeshLod`] is the answer:
//! one [`LodOrigin`] per level, saying which of the two produced it and, when
//! it was the file, which node it came out of. Nothing is substituted
//! silently — that phrase is the plan's, and this module's whole obligation.
//!
//! # Hand-authored levels are never fed to the generator
//!
//! This is the load-bearing half and the easy one to get wrong. The two kinds
//! of level are not interchangeable geometry that happens to have two
//! provenances; they are different structures, and the plan says so: "a
//! hand-authored level is a whole-mesh level, so a mesh with hand LODs is
//! selected per instance at those levels even on the `MeshShader` path — an
//! artist supplies whole-mesh geometry, not a crack-free cluster hierarchy, and
//! the engine will not pretend otherwise."
//!
//! What would go wrong if it did pretend. A [`ClusterDag`] admits a per-cluster
//! cut because every level of it was built by locking a group's outer boundary
//! before simplifying its interior, so the two sides of any cut share their
//! boundary vertices by construction (see [`mod@crate::cluster_dag`]). A mesh an
//! artist exported has no groups and no locked boundaries: its LOD1 is a
//! separately built surface that agrees with LOD0 nowhere in particular. Cutting
//! between them per cluster would tear along every boundary the cut crossed.
//! So a hand-authored level is never an input to a DAG build and never a level
//! of one, and the two live in different fields of [`MeshLod`] rather than in
//! one array of levels that lost the distinction.
//!
//! # What a mesh with both looks like
//!
//! [`MeshLod::levels`] is the chain, coarsening from LOD0. A hand level is
//! [`LodOrigin::Hand`], and its geometry is the glTF mesh it names — every
//! primitive of it, unread and uncopied by this module. A generated level is
//! [`LodOrigin::Generated`], and its geometry is a **uniform cut** through
//! [`MeshLod::dags`]: one DAG per primitive of the base mesh, and the named
//! depth in each. The plan's "What the fallback paths do" is exactly that
//! reading — "`IndirectCount` and `IndirectPerBatch` select a uniform cut …
//! which is exactly a chain level" — so a generated level is whole-mesh too the
//! moment it stands in a chain beside a hand one.
//!
//! The consequence for the renderer, stated so it is not rediscovered: a mesh
//! with any hand-authored level is selected **per instance**. The per-cluster
//! descent is available only for [`MeshLod::dags`], and only where the whole
//! chain came from there.
//!
//! # Where the generator's levels come from, and why there are no ratios here
//!
//! `docs/plan/25-lod.md` still describes the generated chain as ratios — "~50/
//! 25/12/6%, per-asset overridable in sidecar meta RON" — and
//! [`DEFAULT_LOD_RATIOS`](crate::DEFAULT_LOD_RATIOS) is that chain. Those ratios
//! belong to [`build_lod_chain`](crate::build_lod_chain), the chain builder the
//! DAG replaced. **A DAG has no ratio parameter**: each level of it simplifies
//! each group to roughly half its triangles, so the halvings the default ratios
//! spell out are what the structure does by construction, and level `k` of the
//! DAG is the level ratio `2^-k` names. There is nothing here for a per-asset
//! ratio to override, and no sidecar is read — see `docs/backlog.md`.
//!
//! # This is resolution and nothing else
//!
//! No sidecar meta, no `crcbl lod stats`, no editor panel, no bake cache and no
//! upload; nothing in the engine calls this yet. Skinned meshes resolve like any
//! other — the hierarchy is over the bind pose, and nothing here skins.

use std::collections::BTreeMap;

use crate::cluster_dag::{ClusterDag, ClusterDagError, build_cluster_dag};
use crate::gltf_import::GltfScene;

/// The separator between a base name and its level number, per
/// `docs/plan/25-lod.md`'s `name_LOD1`.
///
/// Matched exactly, case included. A convention that also accepted `_lod1` and
/// `_Lod1` would turn every node whose name happens to end that way into a
/// detail level of something, and the plan writes one spelling.
const LEVEL_SUFFIX: &str = "_LOD";

/// How a level's geometry reached the chain.
///
/// The answer to "no silent substitution": every level of
/// [`MeshLod::levels`] carries one of these, so a caller — `crcbl lod stats`,
/// the editor's LOD panel, a bake step deciding what to hash — can always say
/// whether it is looking at what the artist shipped or at what the engine made
/// up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LodOrigin {
    /// The file's own geometry, used verbatim.
    ///
    /// The geometry is `scene.meshes()[mesh]`, every primitive of it, exactly
    /// as [`import_gltf`](crate::import_gltf) read it. This module copies,
    /// decimates and re-clusters none of it.
    Hand {
        /// Which of [`GltfScene::nodes`] this level came out of. LOD0's is the
        /// node [`resolve_lod`] was asked about.
        node: usize,
        /// Which of [`GltfScene::meshes`] that node draws.
        mesh: usize,
        /// What connected it to the chain.
        via: HandLodLink,
    },

    /// Built by [`build_cluster_dag`], and drawn as a uniform cut.
    Generated {
        /// The depth to read in every one of [`MeshLod::dags`]: this level's
        /// geometry is `dags[p].levels()[dag_level]` for each primitive `p` of
        /// the base mesh.
        dag_level: usize,
    },
}

/// What tied a hand-authored level to the mesh it is a level of.
///
/// Recorded rather than resolved away, because the two conventions can be
/// present at once and a report that said only "hand-authored" would not let
/// anyone find where the level was declared.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandLodLink {
    /// LOD0: nothing linked it, it is the mesh itself.
    Base,
    /// A node named `<base>_LOD<n>`.
    NodeName,
    /// The base node's `MSFT_lod` `ids`, whose `n`th entry is LOD`n+1`.
    MsftLod,
    /// Both, agreeing on the same node.
    NodeNameAndMsftLod,
}

/// One mesh's resolved LOD chain, and the DAGs any generated level reads.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshLod {
    levels: Vec<LodOrigin>,
    dags: Vec<ClusterDag>,
}

impl MeshLod {
    /// The chain, LOD0 first, coarsening. Never empty: LOD0 is the mesh the
    /// resolved node draws, and it is always [`LodOrigin::Hand`] — the file's
    /// own geometry is what every other level is an alternative to.
    #[inline]
    #[must_use]
    pub fn levels(&self) -> &[LodOrigin] {
        &self.levels
    }

    /// One [`ClusterDag`] per primitive of the base mesh, parallel to
    /// `scene.meshes()[base].primitives()`, or empty.
    ///
    /// **Empty exactly when the generator was never run**, which is the
    /// assertion `docs/plan/25-lod.md`'s "a fully hand-authored chain is never
    /// touched by the generator" comes down to. Non-empty does not imply a
    /// generated level exists: a mesh of a single cluster admits no level above
    /// its base, so the generator can run and produce a chain of one.
    #[inline]
    #[must_use]
    pub fn dags(&self) -> &[ClusterDag] {
        &self.dags
    }
}

/// Why a LOD chain could not be resolved.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum LodResolveError {
    /// [`resolve_lod`] was given a node index the scene does not have.
    #[error("node {node} was asked for, and the scene has {nodes}")]
    NoSuchNode {
        /// The index asked for.
        node: usize,
        /// How many nodes the scene has.
        nodes: usize,
    },

    /// The node draws no mesh, so there is nothing to have levels of.
    #[error("node {node} draws no mesh")]
    NoMesh {
        /// The node asked about.
        node: usize,
    },

    /// Two different nodes both claim one level.
    ///
    /// Refused rather than ranked. The naming convention and `MSFT_lod` are
    /// alternatives in the plan, not a precedence order, so a file where they
    /// disagree has no answer this code is entitled to pick — and picking one
    /// would be the silent substitution the whole module exists to avoid.
    #[error("LOD{level} is claimed by node {node} and by node {other}")]
    Conflict {
        /// Which level was claimed twice.
        level: usize,
        /// One claimant.
        node: usize,
        /// The other.
        other: usize,
    },

    /// An `MSFT_lod` entry names a node that draws no mesh.
    ///
    /// An explicit declaration that resolves to no geometry, which is refused
    /// where a *named* node that draws nothing is only skipped: `MSFT_lod` says
    /// "this is the level", and a name says "this looks like the level".
    #[error("LOD{level} is MSFT_lod node {node}, which draws no mesh")]
    LevelDrawsNothing {
        /// Which level was declared.
        level: usize,
        /// The node the extension named.
        node: usize,
    },

    /// A gap between hand-authored levels that the generator could not fill.
    ///
    /// The DAG stops adding levels once grouping no longer shrinks the cluster
    /// count, so a small mesh whose hand levels skip ahead of it leaves a hole
    /// in the middle of the chain. Refused for the same reason as
    /// [`Conflict`](Self::Conflict): the alternatives are a chain with a hole
    /// in it or a coarser level quietly standing in for a finer one.
    #[error("LOD{level} has to be generated, and the DAG has {dag_levels} levels")]
    Ungenerated {
        /// The level that could not be filled.
        level: usize,
        /// How many levels the shallowest of the base mesh's DAGs has.
        dag_levels: usize,
    },

    /// The generator refused the base mesh's arrays.
    #[error(transparent)]
    Dag(#[from] ClusterDagError),
}

/// Resolve node `node`'s LOD chain: its hand-authored levels, and generated
/// levels wherever it has none.
///
/// `node` indexes [`GltfScene::nodes`], which is where
/// [`GltfInstance::node`](crate::GltfInstance::node) points. It has to be the
/// **base** node — the one drawing LOD0. A node that is itself somebody's LOD1
/// is treated as a base of its own, because nothing in either convention points
/// upwards.
///
/// The two conventions, both relative to that node:
///
/// - **Node naming.** With the base node named `stem` — or `stem_LOD0`, which
///   is stripped — any node named `stem_LOD<n>` for a decimal `n` above zero is
///   LOD`n`. The digits are matched exactly, so `stem_LOD01` is not LOD1 and
///   `stem_lod1` is not anything. A node matching the pattern that draws no
///   mesh is skipped with a warning rather than refused: the name is a
///   convention, and an empty pivot called `stem_LOD1` is a coincidence a file
///   is allowed to contain.
/// - **`MSFT_lod`.** The `n`th entry of the base node's
///   [`lod_nodes`](crate::GltfNode::lod_nodes) is LOD`n+1`. These are the
///   extension's own words and it is an explicit declaration, so an entry that
///   draws no mesh is [`LodResolveError::LevelDrawsNothing`].
///
/// The generator runs only where a level is missing. Its levels are uniform
/// cuts through [`MeshLod::dags`]; see the [module docs](self) for the reading,
/// and for why a hand-authored level is never one of them.
///
/// ```
/// # use crcbl_scene::{GltfScene, LodOrigin, resolve_lod};
/// # fn report(scene: &GltfScene) -> Result<(), crcbl_scene::LodResolveError> {
/// for instance in scene.instances() {
///     let lod = resolve_lod(scene, instance.node())?;
///     for (level, origin) in lod.levels().iter().enumerate() {
///         match *origin {
///             LodOrigin::Hand { node, .. } => {
///                 println!("LOD{level}: {:?}", scene.nodes()[node].name());
///             }
///             LodOrigin::Generated { dag_level } => {
///                 println!("LOD{level}: generated, DAG level {dag_level}");
///             }
///         }
///     }
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// [`LodResolveError::NoSuchNode`] and [`LodResolveError::NoMesh`] for a node
/// that cannot have a chain; [`LodResolveError::Conflict`] and
/// [`LodResolveError::LevelDrawsNothing`] for a file whose declarations do not
/// resolve; [`LodResolveError::Ungenerated`] for a gap the generator cannot
/// reach; [`LodResolveError::Dag`] for arrays the generator refuses.
///
/// # Panics
///
/// It does not, on any scene [`import_gltf`](crate::import_gltf) produced.
pub fn resolve_lod(scene: &GltfScene, node: usize) -> Result<MeshLod, LodResolveError> {
    let base = scene.nodes().get(node).ok_or(LodResolveError::NoSuchNode {
        node,
        nodes: scene.nodes().len(),
    })?;
    let base_mesh = base.mesh().ok_or(LodResolveError::NoMesh { node })?;

    let hand = hand_levels(scene, node)?;
    let mut levels = vec![LodOrigin::Hand {
        node,
        mesh: base_mesh,
        via: HandLodLink::Base,
    }];

    let deepest = hand.keys().copied().next_back().unwrap_or(0);
    // A chain the artist supplied whole: hand levels with no gap below the
    // deepest one. Nothing is missing, so the generator is never called, and
    // `dags` stays empty to say so.
    if deepest > 0 && (1..=deepest).all(|level| hand.contains_key(&level)) {
        levels.extend((1..=deepest).map(|level| LodOrigin::from(hand[&level])));
        return Ok(MeshLod {
            levels,
            dags: Vec::new(),
        });
    }

    let dags = scene.meshes()[base_mesh]
        .primitives()
        .iter()
        .map(|primitive| build_cluster_dag(primitive.positions(), primitive.indices()))
        .collect::<Result<Vec<_>, _>>()?;
    // The shallowest, because a level of the chain is one depth read in every
    // primitive's DAG: a cut that ran off the end of one of them is not a level
    // of the mesh.
    let dag_levels = dags
        .iter()
        .map(|dag| dag.levels().len())
        .min()
        .unwrap_or_default();

    // With hand levels present the chain is as deep as the artist went, and the
    // generator only fills what is missing below that. With none, it is as deep
    // as the generator reached — one level of chain per level of DAG above the
    // base, so `dag_levels - 1`.
    let depth = if deepest > 0 {
        deepest
    } else {
        dag_levels.saturating_sub(1)
    };
    for level in 1..=depth {
        levels.push(fill(&hand, level, dag_levels)?);
    }

    Ok(MeshLod { levels, dags })
}

/// One level of the chain: the hand-authored one if there is one, else the
/// DAG's cut at that depth.
fn fill(
    hand: &BTreeMap<usize, HandLevel>,
    level: usize,
    dag_levels: usize,
) -> Result<LodOrigin, LodResolveError> {
    if let Some(&origin) = hand.get(&level) {
        return Ok(origin.into());
    }
    if level < dag_levels {
        Ok(LodOrigin::Generated { dag_level: level })
    } else {
        Err(LodResolveError::Ungenerated { level, dag_levels })
    }
}

/// Every hand-authored level `node` has, by level number.
fn hand_levels(
    scene: &GltfScene,
    node: usize,
) -> Result<BTreeMap<usize, HandLevel>, LodResolveError> {
    let mut hand: BTreeMap<usize, HandLevel> = BTreeMap::new();

    if let Some(name) = scene.nodes()[node].name() {
        // Only `_LOD0` is stripped. A base node called `car_LOD1` keeps that
        // whole name as its stem, because nothing in either convention points
        // upwards and guessing that it meant `car` would silently adopt a
        // sibling chain.
        let stem = match strip_level(name) {
            Some((stem, 0)) => stem,
            _ => name,
        };
        for (candidate, other) in scene.nodes().iter().enumerate() {
            let Some((other_stem, level)) = other.name().and_then(strip_level) else {
                continue;
            };
            if other_stem != stem || level == 0 || candidate == node {
                continue;
            }
            let Some(mesh) = other.mesh() else {
                crcbl_core::log::warn!(
                    "node {candidate} is named like LOD{level} of node {node} and draws no \
                     mesh, so it is not a level",
                );
                continue;
            };
            claim(&mut hand, level, candidate, mesh, HandLodLink::NodeName)?;
        }
    }

    for (offset, &candidate) in scene.nodes()[node].lod_nodes().iter().enumerate() {
        let level = offset + 1;
        let mesh = scene.nodes()[candidate]
            .mesh()
            .ok_or(LodResolveError::LevelDrawsNothing {
                level,
                node: candidate,
            })?;
        claim(&mut hand, level, candidate, mesh, HandLodLink::MsftLod)?;
    }

    Ok(hand)
}

/// A hand-authored level, before it becomes a [`LodOrigin::Hand`].
#[derive(Clone, Copy, Debug)]
struct HandLevel {
    node: usize,
    mesh: usize,
    via: HandLodLink,
}

impl From<HandLevel> for LodOrigin {
    fn from(level: HandLevel) -> Self {
        Self::Hand {
            node: level.node,
            mesh: level.mesh,
            via: level.via,
        }
    }
}

/// Record node `node` as `level`, or refuse a second claimant.
///
/// Two claims on one level agree only when they are the same node; then the
/// level keeps both links, because a report that dropped one would send a
/// reader looking in the wrong half of the file.
fn claim(
    hand: &mut BTreeMap<usize, HandLevel>,
    level: usize,
    node: usize,
    mesh: usize,
    via: HandLodLink,
) -> Result<(), LodResolveError> {
    match hand.get_mut(&level) {
        None => {
            hand.insert(level, HandLevel { node, mesh, via });
            Ok(())
        }
        Some(held) if held.node == node => {
            if held.via != via {
                held.via = HandLodLink::NodeNameAndMsftLod;
            }
            Ok(())
        }
        Some(held) => Err(LodResolveError::Conflict {
            level,
            node,
            other: held.node,
        }),
    }
}

/// `name` split at its `_LOD<n>` suffix, if it has a well-formed one.
///
/// The digits have to be the level's canonical decimal spelling, so `x_LOD01`
/// is not a level of `x` — otherwise it and `x_LOD1` would both be LOD1 and one
/// of them would have to lose.
fn strip_level(name: &str) -> Option<(&str, usize)> {
    let cut = name.rfind(LEVEL_SUFFIX)?;
    let (stem, digits) = name.split_at(cut);
    let digits = &digits[LEVEL_SUFFIX.len()..];
    let level: usize = digits.parse().ok()?;
    (level.to_string() == digits).then_some((stem, level))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf_fixture::{LodNode, import_glb_bytes, lod_glb};
    use crate::simplify::tests::torus;
    use crcbl_assets::StorageError;

    /// A hand-authored level, spelled the way the assertions below read best.
    const fn hand(node: usize, mesh: usize, via: HandLodLink) -> LodOrigin {
        LodOrigin::Hand { node, mesh, via }
    }

    /// Import a document of `nodes`, with every node in the scene.
    fn scene_of(nodes: &[LodNode<'_>]) -> GltfScene {
        let all: Vec<usize> = (0..nodes.len()).collect();
        import_glb_bytes(&lod_glb(nodes, &all)).unwrap()
    }

    /// A distinguishable mesh per level: four tori, each coarser than the one
    /// before, and none of them anything the generator would produce.
    fn tori() -> [(Vec<[f32; 3]>, Vec<u32>); 4] {
        [torus(16, 12), torus(12, 8), torus(8, 6), torus(6, 4)]
    }

    /// Asserts `level`'s geometry is `mesh`'s, byte for byte, and — the half
    /// that stops the trivial pass — that it is not *any* level `generated`
    /// holds. Without the second half an importer that ignored hand levels
    /// entirely and generated the whole chain would pass: the chain would
    /// exist, it would coarsen, and nothing would notice the geometry was not
    /// the file's.
    fn assert_verbatim(
        scene: &GltfScene,
        lod: &MeshLod,
        level: usize,
        mesh: &(Vec<[f32; 3]>, Vec<u32>),
        generated: &ClusterDag,
    ) {
        let LodOrigin::Hand { mesh: index, .. } = lod.levels()[level] else {
            panic!("LOD{level} is {:?}, not hand-authored", lod.levels()[level]);
        };
        let primitive = &scene.meshes()[index].primitives()[0];
        assert_eq!(primitive.positions(), mesh.0, "LOD{level}'s positions");
        assert_eq!(primitive.indices(), mesh.1, "LOD{level}'s indices");
        for (depth, cut) in generated.levels().iter().enumerate() {
            assert_ne!(
                primitive.positions(),
                cut.positions(),
                "LOD{level} is what the generator makes at depth {depth}, so this \
                 fixture cannot tell a hand level from a generated one"
            );
        }
    }

    #[test]
    fn a_full_hand_authored_chain_is_verbatim_and_the_generator_never_runs() {
        let [base, lod1, lod2] = {
            let [a, b, c, _] = tori();
            [a, b, c]
        };
        let scene = scene_of(&[
            LodNode::new("car", &base),
            LodNode::new("car_LOD1", &lod1),
            LodNode::new("car_LOD2", &lod2),
        ]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert_eq!(
            lod.levels(),
            [
                hand(0, 0, HandLodLink::Base),
                hand(1, 1, HandLodLink::NodeName),
                hand(2, 2, HandLodLink::NodeName),
            ]
        );
        assert!(
            lod.dags().is_empty(),
            "the generator ran on a chain that was already complete"
        );

        let generated = build_cluster_dag(&base.0, &base.1).unwrap();
        assert!(
            generated.levels().len() > 2,
            "the fixture has to have a generated level to \
             differ from at every hand level"
        );
        assert_verbatim(&scene, &lod, 1, &lod1, &generated);
        assert_verbatim(&scene, &lod, 2, &lod2, &generated);
    }

    #[test]
    fn a_mesh_with_no_hand_levels_is_generated_all_the_way_down() {
        let base = torus(16, 12);
        let scene = scene_of(&[LodNode::new("car", &base)]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert_eq!(
            lod.dags().len(),
            1,
            "one DAG per primitive of the base mesh"
        );
        let depth = lod.dags()[0].levels().len();
        assert!(depth > 2, "{depth} DAG levels is not a chain worth testing");
        assert_eq!(lod.levels()[0], hand(0, 0, HandLodLink::Base));
        assert_eq!(
            lod.levels()[1..],
            (1..depth)
                .map(|dag_level| LodOrigin::Generated { dag_level })
                .collect::<Vec<_>>(),
            "every level below LOD0 is a uniform cut, at its own depth"
        );
        assert_eq!(
            lod.dags()[0],
            build_cluster_dag(&base.0, &base.1).unwrap(),
            "and the DAG is the one the builder makes from the base arrays"
        );
    }

    #[test]
    fn a_gap_between_hand_levels_is_the_only_thing_generated() {
        let [base, lod1, _, lod3] = tori();
        let scene = scene_of(&[
            LodNode::new("car", &base),
            LodNode::new("car_LOD1", &lod1),
            LodNode::new("car_LOD3", &lod3),
        ]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert_eq!(
            lod.levels(),
            [
                hand(0, 0, HandLodLink::Base),
                hand(1, 1, HandLodLink::NodeName),
                LodOrigin::Generated { dag_level: 2 },
                hand(2, 2, HandLodLink::NodeName),
            ],
            "LOD2 was the only level missing, and the chain stops where the \
             artist stopped"
        );

        let generated = build_cluster_dag(&base.0, &base.1).unwrap();
        assert_verbatim(&scene, &lod, 1, &lod1, &generated);
        assert_verbatim(&scene, &lod, 3, &lod3, &generated);
        let cut = &lod.dags()[0].levels()[2];
        assert_eq!(cut, &generated.levels()[2], "the filled level is the DAG's");
        for hand_mesh in [&lod1, &lod3] {
            assert_ne!(
                cut.positions(),
                hand_mesh.0,
                "the generated level is not one of the hand meshes"
            );
        }
        assert!(
            cut.indices().len() < base.1.len(),
            "and it is coarser than the base"
        );
    }

    /// The extension's lower levels are deliberately outside every scene, so a
    /// resolver reading [`GltfScene::instances`] would find none of them.
    #[test]
    fn msft_lod_names_its_levels_by_node_and_they_need_not_be_in_a_scene() {
        let [base, lod1, lod2, _] = tori();
        let scene = import_glb_bytes(&lod_glb(
            &[
                LodNode::new("car", &base).msft_lod(r#"{ "ids": [1, 2] }"#),
                LodNode::new("cheap", &lod1),
                LodNode::new("cheaper", &lod2),
            ],
            &[0],
        ))
        .unwrap();

        assert_eq!(scene.instances().len(), 1, "only the base node is drawn");
        assert_eq!(
            scene.nodes().len(),
            3,
            "and all three are in the node table"
        );

        let lod = resolve_lod(&scene, 0).unwrap();

        assert_eq!(
            lod.levels(),
            [
                hand(0, 0, HandLodLink::Base),
                hand(1, 1, HandLodLink::MsftLod),
                hand(2, 2, HandLodLink::MsftLod),
            ],
            "the names say nothing here, so only the extension can have found them"
        );
        assert!(lod.dags().is_empty());
        let generated = build_cluster_dag(&base.0, &base.1).unwrap();
        assert_verbatim(&scene, &lod, 1, &lod1, &generated);
        assert_verbatim(&scene, &lod, 2, &lod2, &generated);
    }

    #[test]
    fn a_level_both_conventions_agree_on_records_both() {
        let [base, lod1, _, _] = tori();
        let scene = scene_of(&[
            LodNode::new("car", &base).msft_lod(r#"{ "ids": [1] }"#),
            LodNode::new("car_LOD1", &lod1),
        ]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert_eq!(
            lod.levels(),
            [
                hand(0, 0, HandLodLink::Base),
                hand(1, 1, HandLodLink::NodeNameAndMsftLod),
            ]
        );
        assert!(lod.dags().is_empty());
    }

    #[test]
    fn two_nodes_claiming_one_level_are_refused() {
        let [base, lod1, other, _] = tori();
        let scene = scene_of(&[
            LodNode::new("car", &base).msft_lod(r#"{ "ids": [2] }"#),
            LodNode::new("car_LOD1", &lod1),
            LodNode::new("something_else", &other),
        ]);

        assert_eq!(
            resolve_lod(&scene, 0).unwrap_err(),
            LodResolveError::Conflict {
                level: 1,
                node: 2,
                other: 1
            }
        );
    }

    #[test]
    fn the_naming_convention_is_matched_exactly() {
        assert_eq!(strip_level("car_LOD1"), Some(("car", 1)));
        assert_eq!(strip_level("car_LOD0"), Some(("car", 0)));
        assert_eq!(strip_level("car_LOD12"), Some(("car", 12)));
        assert_eq!(strip_level("a_LOD1_LOD2"), Some(("a_LOD1", 2)));
        for not_a_level in ["car_LOD01", "car_lod1", "car_LOD", "car_LODx", "car", ""] {
            assert_eq!(strip_level(not_a_level), None, "{not_a_level}");
        }
    }

    #[test]
    fn a_name_that_is_not_the_convention_is_not_a_level() {
        let [base, lod1, _, _] = tori();
        let scene = scene_of(&[LodNode::new("car", &base), LodNode::new("car_lod1", &lod1)]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert!(
            lod.levels()[1..]
                .iter()
                .all(|origin| matches!(origin, LodOrigin::Generated { .. })),
            "car_lod1 was adopted as a level: {:?}",
            lod.levels()
        );
        assert!(!lod.dags().is_empty(), "so the generator had to run");
    }

    #[test]
    fn a_base_node_named_lod0_is_the_stem_of_its_own_chain() {
        let [base, lod1, _, _] = tori();
        let scene = scene_of(&[
            LodNode::new("car_LOD0", &base),
            LodNode::new("car_LOD1", &lod1),
        ]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert_eq!(
            lod.levels(),
            [
                hand(0, 0, HandLodLink::Base),
                hand(1, 1, HandLodLink::NodeName)
            ]
        );
        assert!(lod.dags().is_empty());
    }

    /// A name is a convention and an empty pivot can share one by accident, so
    /// this is skipped rather than refused — unlike the `MSFT_lod` case below.
    #[test]
    fn a_node_named_like_a_level_that_draws_nothing_is_not_a_level() {
        let base = torus(16, 12);
        let scene = scene_of(&[LodNode::new("car", &base), LodNode::empty("car_LOD1")]);

        let lod = resolve_lod(&scene, 0).unwrap();

        assert!(
            lod.levels()[1..]
                .iter()
                .all(|origin| matches!(origin, LodOrigin::Generated { .. })),
            "a node with no mesh became a level: {:?}",
            lod.levels()
        );
    }

    #[test]
    fn an_msft_lod_level_that_draws_nothing_is_refused() {
        let base = torus(16, 12);
        let scene = scene_of(&[
            LodNode::new("car", &base).msft_lod(r#"{ "ids": [1] }"#),
            LodNode::empty("cheap"),
        ]);

        assert_eq!(
            resolve_lod(&scene, 0).unwrap_err(),
            LodResolveError::LevelDrawsNothing { level: 1, node: 1 },
            "an explicit declaration that resolves to no geometry"
        );
    }

    /// A mesh of one cluster has no level above its base, so a hand chain that
    /// skips ahead of it leaves a hole nothing can fill.
    #[test]
    fn a_gap_the_generator_cannot_reach_is_refused() {
        let triangle = (
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        );
        let scene = scene_of(&[
            LodNode::new("flat", &triangle),
            LodNode::new("flat_LOD2", &triangle),
        ]);

        assert_eq!(
            build_cluster_dag(&triangle.0, &triangle.1)
                .unwrap()
                .levels()
                .len(),
            1,
            "the fixture only says anything while the DAG stops at its base"
        );
        assert_eq!(
            resolve_lod(&scene, 0).unwrap_err(),
            LodResolveError::Ungenerated {
                level: 1,
                dag_levels: 1
            }
        );
    }

    #[test]
    fn a_node_that_is_not_there_or_draws_nothing_has_no_chain() {
        let base = torus(8, 6);
        let scene = scene_of(&[LodNode::new("car", &base), LodNode::empty("pivot")]);

        assert_eq!(
            resolve_lod(&scene, 5).unwrap_err(),
            LodResolveError::NoSuchNode { node: 5, nodes: 2 }
        );
        assert_eq!(
            resolve_lod(&scene, 1).unwrap_err(),
            LodResolveError::NoMesh { node: 1 }
        );
    }

    /// Every id is checked at import, so a chain can never quietly lose a level
    /// the file declared.
    #[test]
    fn a_malformed_msft_lod_makes_the_document_malformed() {
        let base = torus(8, 6);
        for ids in [
            r#"{ "ids": [7] }"#,
            r#"{ "ids": [-1] }"#,
            r#"{ "ids": ["1"] }"#,
            r#"{ "ids": 1 }"#,
            r#"{ }"#,
            r#"[1]"#,
        ] {
            let refused =
                import_glb_bytes(&lod_glb(&[LodNode::new("car", &base).msft_lod(ids)], &[0]))
                    .unwrap_err();

            assert!(
                matches!(&refused, StorageError::Other(reason) if reason.contains("MSFT_lod")),
                "{ids} was refused as {refused:?}"
            );
        }
    }
}
