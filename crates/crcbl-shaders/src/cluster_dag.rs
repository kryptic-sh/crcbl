//! A mesh's cluster DAG, in the cooked form the renderer receives it in.
//!
//! `docs/plan/25-lod.md`'s "How a DAG reaches the renderer" decided this shape.
//! The builder is `crcbl_scene::cluster_dag::build_cluster_dag`, and neither
//! this crate nor `crcbl-render` can call it: `crcbl-scene` depends on this one,
//! and the renderer must not depend on `crcbl-scene` at all — that would pull
//! `gltf` into the renderer. So the DAG arrives the way the compiled SPIR-V
//! beside it does, **as a committed artifact with a generator and a check**:
//!
//! * `tools/cook-clusters.rs` builds the DAG from the builder and writes
//!   `clusters/dunes.dag`;
//! * `--check` rebuilds it and demands the bytes be identical, and CI runs that,
//!   exactly as it runs `tools/compile-shaders.sh --check`;
//! * this module is the decoder, and the only thing a consumer sees.
//!
//! When topic 6's asset pipeline arrives it replaces the generator, not the
//! consumer.
//!
//! # What a cut is, and how a camera picks one
//!
//! Selection picks a **cut**: a set of clusters covering the surface exactly
//! once, formed by expanding some groups (draw their children) and not others
//! (draw their parents). Writing `E(G)` for
//! [`ClusterGroup::projected_error`](crate::cluster_dag::ClusterGroup::projected_error)`
//! > budget` — *this group is expanded* — a
//! cluster is drawn exactly when
//!
//! ```text
//! !E(the group that produced it) && E(the group that contains it)
//! ```
//!
//! reading a level-0 cluster's absent producer as never expanded and a top-level
//! cluster's absent container as always expanded. Both halves ask about a
//! **group**, never about the cluster's own centre, which is what lets a GPU
//! evaluate the rule per cluster with no communication and still move every
//! cluster of a group together. The reasoning — and the crack a per-cluster
//! distance term opens — is in `crcbl_scene::cluster_dag`'s module docs; this
//! module carries the numbers that rule reads and
//! `the_cooked_dag_draws_a_crack_free_cut_from_every_camera` is it, run over the
//! committed artifact.
//!
//! # This is the data, not the descent
//!
//! Nothing in the engine selects on any of this yet. The amplification-stage
//! descent, the hysteresis and the upload are the next slice; what is here is
//! the DAG reaching a crate the renderer can see, and the metric its two
//! per-group numbers exist to be fed to.

use crate::dunes;
use crate::meshlet::{MESHLET_STRIDE, MeshClusters, Meshlet};

/// The committed DAG of [`crate::dunes`], decoded.
///
/// The artifact does not repeat level 0's positions — `build_cluster_dag`
/// documents level 0 as the caller's own array verbatim, so they are
/// [`dunes::positions`] and re-committing them would be a second copy to drift.
/// `tools/cook-clusters.rs` asserts the two agree before it writes anything.
///
/// # Panics
///
/// If the committed artifact does not decode, which is a corrupt file in the
/// tree rather than a runtime condition: it is generated, checked byte for byte
/// by CI, and read by [`include_bytes!`] from this crate's own source.
#[must_use]
pub fn dunes_dag() -> ClusterDag {
    ClusterDag::from_bytes(include_bytes!("../clusters/dunes.dag"), dunes::positions())
        .unwrap_or_else(|error| panic!("the committed dunes DAG does not decode: {error}"))
}

/// The sphere a group's error is projected from.
///
/// Separate from [`ClusterBounds`](crate::meshlet::ClusterBounds), which a
/// cluster carries for culling: that one bounds a single cluster's geometry as
/// tightly as the builder can, where this one bounds a whole group's and is
/// deliberately grown to contain every sphere below it in the DAG. **Only this
/// one may decide a level** — the containment is what carries monotone stored
/// error into monotone projected error, and a cluster's own tighter sphere would
/// break it.
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupBounds {
    /// Centre of the sphere, in the mesh's own space.
    pub center: [f32; 3],
    /// Distance from [`center`](Self::center) to the furthest point the sphere
    /// has to contain. Never negative.
    pub radius: f32,
}

impl GroupBounds {
    /// Whether this sphere contains `inner` whole.
    ///
    /// The relation the descent depends on: a sphere containing another is never
    /// further from any eye than the one it contains.
    #[must_use]
    pub fn contains(&self, inner: Self) -> bool {
        let separation = f64::from(
            [0, 1, 2]
                .map(|axis| self.center[axis] - inner.center[axis])
                .iter()
                .map(|delta| delta * delta)
                .sum::<f32>()
                .sqrt(),
        );
        separation + f64::from(inner.radius) <= f64::from(self.radius)
    }
}

/// A group of neighbouring clusters, the clusters simplifying it produced, and
/// what that simplification costs.
///
/// The DAG's edges live here rather than on a cluster, because grouping is what
/// relates two levels and a cluster's parents are *the group's* parents — all of
/// them. The cost lives here for the second reason this module's docs give: a
/// group simplifies as a unit, so one error and one sphere are what every
/// cluster it touches has to be judged by.
///
/// `PartialEq` but not `Eq`: it holds floats.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterGroup {
    /// The clusters that were grouped, as indices into the clusters of the level
    /// this group belongs to. Ascending, and never empty.
    pub children: Vec<u32>,
    /// The clusters re-splitting the simplified group produced, as indices into
    /// the clusters of the level **above** this group's. Ascending and
    /// contiguous.
    pub parents: Vec<u32>,
    /// How far this group's simplification may have moved the surface, in the
    /// mesh's own units of length.
    ///
    /// A quadric error, and **not a certified Hausdorff bound** — see
    /// `crcbl_scene::Simplified::max_error` for what it does and does not claim.
    pub error: f32,
    /// The sphere [`error`](Self::error) is projected from.
    pub bounds: GroupBounds,
}

impl ClusterGroup {
    /// What this group's simplification would cost on screen, in pixels, from an
    /// eye at `eye`.
    ///
    /// `pixels_per_unit` is how many pixels one unit of length subtends one unit
    /// from the eye — `0.5 * viewport_height / tan(0.5 * fov_y)` for a
    /// perspective camera — so the result is [`error`](Self::error) scaled by
    /// that and divided by the distance to the nearest point of
    /// [`bounds`](Self::bounds). Over a pixel budget means descend into this
    /// group's children; at or under means its parents are close enough.
    ///
    /// **Both arguments are in the mesh's own space**, which is where the bounds
    /// are. An instance's eye is the camera put through the inverse of its
    /// transform, and a uniform scale on that transform belongs in
    /// `pixels_per_unit`; a non-uniform one does not survive a bounding sphere
    /// and is not something this metric can express.
    ///
    /// [`f32::INFINITY`] when the eye is inside the sphere: there is no distance
    /// to divide by, and "as close as possible, so descend" is both the
    /// conservative answer and the monotone one, since an eye inside a sphere is
    /// inside every sphere containing it.
    ///
    /// This is `crcbl_scene::ClusterGroup::projected_error` on the cooked side of
    /// the seam, and `tools/cook-clusters.rs` compares the two over the whole
    /// DAG at a sweep of eyes rather than letting a second copy drift.
    #[must_use]
    pub fn projected_error(&self, eye: [f32; 3], pixels_per_unit: f32) -> f32 {
        let separation = [0, 1, 2]
            .map(|axis| eye[axis] - self.bounds.center[axis])
            .iter()
            .map(|delta| delta * delta)
            .sum::<f32>()
            .sqrt();
        let distance = separation - self.bounds.radius;
        if distance <= 0.0 {
            return f32::INFINITY;
        }
        self.error * pixels_per_unit / distance
    }
}

/// One level of the DAG: a mesh, its clusters, their errors, and the grouping
/// that produced the level above.
#[derive(Clone, Debug, PartialEq)]
pub struct DagLevel {
    /// This level's vertex positions. Level 0's are the base mesh's verbatim.
    ///
    /// Positions alone, because that is all the decimator carries: a coarser
    /// level's vertices are wherever collapses put them and belong to no vertex
    /// of the level below. [`dunes::vertex_at`] is what turns one into a
    /// [`MeshVertex`](crate::mesh::MeshVertex), by evaluating the surface rather
    /// than by interpolating an attribute nothing recorded.
    pub positions: Vec<[f32; 3]>,

    /// This level's clusters and the two arrays they index, built over this
    /// level's own geometry — everything a mesh stage needs, ready to upload.
    pub clusters: MeshClusters,

    /// Per cluster, how far its surface may depart from the base mesh's, in the
    /// mesh's own units of length. Parallel to
    /// [`clusters.clusters`](MeshClusters::clusters).
    ///
    /// **Every cluster a group produced carries the group's number**, not one of
    /// its own: a group simplifies as a unit, so its parents stand or fall
    /// together. Detail still varies across a level because different groups
    /// report different errors; what does not vary is detail *within* a group.
    /// Level 0 is the base mesh untouched and reads zero throughout.
    pub errors: Vec<f32>,

    /// Per cluster, the sphere [`errors`](Self::errors) is projected from — the
    /// bounds of the group that produced it, so parallel to that array and
    /// carrying the same value for every cluster one group produced.
    ///
    /// Level 0 was produced by no group and reads each cluster's own bounding
    /// sphere; nothing selects on it, since a level-0 cluster's producing error
    /// is zero.
    pub bounds: Vec<GroupBounds>,

    /// How this level's clusters were grouped to build the level above. Every
    /// cluster of this level is in exactly one group; empty on the top level.
    pub groups: Vec<ClusterGroup>,
}

/// A mesh as a DAG of clusters: level 0 is the base, each level above it the
/// simplified re-split of the one below.
#[derive(Clone, Debug, PartialEq)]
pub struct ClusterDag {
    /// The levels, finest first. Never empty.
    pub levels: Vec<DagLevel>,
}

/// Why a committed DAG could not be decoded.
///
/// A hand-written `Display` rather than a `thiserror` derive, because this crate
/// has no dependencies at all — the same decision `crate::sha256` and
/// [`MeshletTooLarge`](crate::meshlet::MeshletTooLarge) record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DagDecodeError {
    /// What the decoder was reading when it gave up.
    pub what: &'static str,
    /// Byte offset it had reached.
    pub at: usize,
}

impl std::fmt::Display for DagDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

impl std::error::Error for DagDecodeError {}

/// What every artifact this codec writes begins with, so a file that is not one
/// is refused rather than read as numbers.
const MAGIC: [u8; 8] = *b"CRCBLDAG";

/// The format's version, bumped whenever the byte layout below changes.
///
/// A committed artifact and its decoder move in one commit, so this cannot
/// legitimately mismatch — it is here for the case that does happen: a stale
/// artifact arriving through a merge, which would otherwise decode as garbage
/// counts and allocate on them.
const VERSION: u32 = 1;

impl ClusterDag {
    /// The bytes this DAG is committed as.
    ///
    /// Little-endian scalars throughout, no alignment padding except the corner
    /// array's tail, and every array preceded by its own count — so the decoder
    /// is a forward walk that never seeks and never trusts a length it has not
    /// read. The clusters are written with [`Meshlet::to_bytes`], which is the
    /// layout the GPU reads them in, so the artifact holds the record rather
    /// than a second spelling of it.
    ///
    /// **Level 0's positions are not written.** They are the array the builder
    /// was handed, which the consumer already has; `dunes_dag` supplies
    /// [`dunes::positions`] and the generator asserts the two agree.
    ///
    /// # Panics
    ///
    /// If a level's [`errors`](DagLevel::errors) or [`bounds`](DagLevel::bounds)
    /// is not parallel to its clusters. Those two are the one pair of arrays
    /// written without a count of their own — they are per cluster by
    /// definition — so a mismatch here is an artifact whose errors are read off
    /// by one and a DAG that selects the wrong level everywhere. It is a bake
    /// failure rather than a runtime condition, and it refuses rather than
    /// writing the file.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        push_count(&mut bytes, self.levels.len());
        for (depth, level) in self.levels.iter().enumerate() {
            // Level 0's positions are the caller's own array; every other
            // level's are the decimator's output and have nowhere else to live.
            let positions: &[[f32; 3]] = if depth == 0 { &[] } else { &level.positions };
            push_count(&mut bytes, positions.len());
            for position in positions {
                push_floats(&mut bytes, position);
            }

            assert!(
                level.errors.len() == level.clusters.clusters.len()
                    && level.bounds.len() == level.clusters.clusters.len(),
                "level {depth} has {} clusters but {} errors and {} spheres",
                level.clusters.clusters.len(),
                level.errors.len(),
                level.bounds.len()
            );
            push_count(&mut bytes, level.clusters.clusters.len());
            for cluster in &level.clusters.clusters {
                bytes.extend_from_slice(&cluster.to_bytes());
            }
            push_count(&mut bytes, level.clusters.vertices.len());
            for &vertex in &level.clusters.vertices {
                bytes.extend_from_slice(&vertex.to_le_bytes());
            }
            push_count(&mut bytes, level.clusters.corners.len());
            bytes.extend_from_slice(&crate::meshlet::corner_bytes(&level.clusters.corners));

            push_floats(&mut bytes, &level.errors);
            for sphere in &level.bounds {
                push_sphere(&mut bytes, *sphere);
            }

            push_count(&mut bytes, level.groups.len());
            for group in &level.groups {
                push_count(&mut bytes, group.children.len());
                for &child in &group.children {
                    bytes.extend_from_slice(&child.to_le_bytes());
                }
                push_count(&mut bytes, group.parents.len());
                for &parent in &group.parents {
                    bytes.extend_from_slice(&parent.to_le_bytes());
                }
                bytes.extend_from_slice(&group.error.to_le_bytes());
                push_sphere(&mut bytes, group.bounds);
            }
        }
        bytes
    }

    /// The inverse of [`to_bytes`](Self::to_bytes), with level 0's positions
    /// supplied by the caller.
    ///
    /// # Errors
    ///
    /// [`DagDecodeError`] naming what ran out of bytes, for a truncated or
    /// corrupt artifact. A count that would address past the end of the input is
    /// refused before anything is allocated on it, so a hostile length cannot
    /// reserve memory the file never had.
    ///
    /// Also for a `base_positions` that is not the array this artifact was cooked
    /// over — `check_indices_are_in_range` is what turns that from an
    /// out-of-bounds panic in whoever walks the clusters into a refusal here.
    pub fn from_bytes(bytes: &[u8], base_positions: Vec<[f32; 3]>) -> Result<Self, DagDecodeError> {
        let mut reader = Reader { bytes, at: 0 };
        if reader.take("the magic", MAGIC.len())? != MAGIC {
            return Err(DagDecodeError {
                what: "the magic is not a cooked cluster DAG",
                at: 0,
            });
        }
        let version = reader.u32("the version")?;
        if version != VERSION {
            return Err(DagDecodeError {
                what: "the version is not the one this decoder writes",
                at: MAGIC.len(),
            });
        }

        // Every level writes five counts before anything else, and every group
        // two counts, an error and a sphere. Those are the smallest a level and
        // a group can be, which is what makes the capacity bounds below real
        // ones rather than decoration.
        const LEVEL_MINIMUM: usize = 5 * 4;
        const GROUP_MINIMUM: usize = 2 * 4 + 4 + 16;

        let level_count = reader.count("the level count", LEVEL_MINIMUM)?;
        let mut base_positions = Some(base_positions);
        let mut levels = Vec::with_capacity(level_count);
        for _ in 0..level_count {
            let decoded = reader.array("a position", 3 * 4, |reader| {
                Ok([
                    reader.f32("a position")?,
                    reader.f32("a position")?,
                    reader.f32("a position")?,
                ])
            })?;
            // Level 0 alone was written with no positions, because they are the
            // caller's own array; `base_positions` is therefore taken exactly
            // once and every later level uses what it decoded.
            let positions = match base_positions.take() {
                Some(base) => base,
                None => decoded,
            };

            let clusters = reader.array("a cluster", MESHLET_STRIDE, |reader| {
                let element = reader.take("a cluster", MESHLET_STRIDE)?;
                let element: &[u8; MESHLET_STRIDE] = element
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("take returned the length asked for"));
                Ok(Meshlet::from_bytes(element))
            })?;
            let vertices = reader.array("a cluster vertex", 4, |reader| {
                reader.u32("a cluster vertex")
            })?;
            let corner_count = reader.count("the corner count", 1)?;
            let corners =
                reader.take("the corners", corner_count.div_ceil(4) * 4)?[..corner_count].to_vec();

            let errors = (0..clusters.len())
                .map(|_| reader.f32("a cluster error"))
                .collect::<Result<Vec<f32>, _>>()?;
            let bounds = (0..clusters.len())
                .map(|_| reader.sphere("a cluster sphere"))
                .collect::<Result<Vec<GroupBounds>, _>>()?;

            let group_count = reader.count("the group count", GROUP_MINIMUM)?;
            let mut groups = Vec::with_capacity(group_count);
            for _ in 0..group_count {
                let children =
                    reader.array("a group child", 4, |reader| reader.u32("a group child"))?;
                let parents =
                    reader.array("a group parent", 4, |reader| reader.u32("a group parent"))?;
                groups.push(ClusterGroup {
                    children,
                    parents,
                    error: reader.f32("a group error")?,
                    bounds: reader.sphere("a group sphere")?,
                });
            }

            let level = DagLevel {
                positions,
                clusters: MeshClusters {
                    clusters,
                    vertices,
                    corners,
                },
                errors,
                bounds,
                groups,
            };
            check_indices_are_in_range(&level, reader.at)?;
            levels.push(level);
        }

        if reader.at != bytes.len() {
            return Err(DagDecodeError {
                what: "there are bytes after the last level",
                at: reader.at,
            });
        }
        Ok(Self { levels })
    }
}

/// Every index a decoded level holds names something that level has.
///
/// **This is what makes [`ClusterDag::from_bytes`] safe to hand a wrong
/// `base_positions`.** Level 0's positions come from the caller rather than from
/// the file, so an array of the wrong length decodes without complaint and then
/// panics — or draws another mesh's geometry — in whoever walks the clusters.
/// The three ranges are the three ways that shows up, and each is checked here
/// rather than left as a precondition a caller has no way to satisfy: a
/// cluster's run inside the vertex array, a cluster's corners inside the corner
/// array, and every entry of the run inside the positions.
///
/// `at` is the reader's position, so the error points at the level that failed.
fn check_indices_are_in_range(level: &DagLevel, at: usize) -> Result<(), DagDecodeError> {
    let refuse = |what| Err(DagDecodeError { what, at });
    for cluster in &level.clusters.clusters {
        let run = (cluster.vertex_offset as usize)..;
        let Some(run) = level
            .clusters
            .vertices
            .get(run)
            .and_then(|run| run.get(..cluster.vertex_count as usize))
        else {
            return refuse("a cluster's vertex run is not inside the vertex array");
        };
        if level
            .clusters
            .corners
            .get(cluster.triangle_offset as usize..)
            .and_then(|corners| corners.get(..cluster.triangle_count as usize * 3))
            .is_none()
        {
            return refuse("a cluster's corners are not inside the corner array");
        }
        if run
            .iter()
            .any(|&vertex| vertex as usize >= level.positions.len())
        {
            return refuse("a cluster names a vertex the level does not have");
        }
    }
    // A corner indexes its own cluster's run, so the bound is the run's length
    // and not the array's — which is the whole difference between a corner and
    // an index, and a corner past its run reads a neighbouring cluster's vertex.
    for cluster in &level.clusters.clusters {
        let corners = &level.clusters.corners[cluster.triangle_offset as usize..]
            [..cluster.triangle_count as usize * 3];
        if corners
            .iter()
            .any(|&corner| u32::from(corner) >= cluster.vertex_count)
        {
            return refuse("a corner reaches past its own cluster's vertex run");
        }
    }
    Ok(())
}

fn push_count(bytes: &mut Vec<u8>, count: usize) {
    let count = u32::try_from(count)
        .unwrap_or_else(|_| unreachable!("a bake that produced 4 billion of anything"));
    bytes.extend_from_slice(&count.to_le_bytes());
}

fn push_floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_sphere(bytes: &mut Vec<u8>, sphere: GroupBounds) {
    push_floats(bytes, &sphere.center);
    bytes.extend_from_slice(&sphere.radius.to_le_bytes());
}

/// A forward walk over the artifact that refuses to read past its end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, what: &'static str, length: usize) -> Result<&'a [u8], DagDecodeError> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(DagDecodeError { what, at: self.at })?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(DagDecodeError { what, at: self.at })?;
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, DagDecodeError> {
        let word = self.take(what, 4)?;
        Ok(u32::from_le_bytes(word.try_into().unwrap_or_else(|_| {
            unreachable!("take returned the length asked for")
        })))
    }

    fn f32(&mut self, what: &'static str) -> Result<f32, DagDecodeError> {
        Ok(f32::from_bits(self.u32(what)?))
    }

    fn sphere(&mut self, what: &'static str) -> Result<GroupBounds, DagDecodeError> {
        Ok(GroupBounds {
            center: [self.f32(what)?, self.f32(what)?, self.f32(what)?],
            radius: self.f32(what)?,
        })
    }

    /// A count, refused unless `stride` bytes of it each could still be read.
    ///
    /// **The bound is the point**: without it a corrupt count reserves capacity
    /// the file never had, which is an allocation decided by the file rather
    /// than by the reader. It is not a validity check — the elements are read
    /// and can still run out — it is what keeps the failure a decode error
    /// instead of an out-of-memory abort.
    fn count(&mut self, what: &'static str, stride: usize) -> Result<usize, DagDecodeError> {
        let count = self.u32(what)? as usize;
        let available = self.bytes.len() - self.at;
        if count.saturating_mul(stride) > available {
            return Err(DagDecodeError { what, at: self.at });
        }
        Ok(count)
    }

    /// A counted array of `stride`-byte elements.
    fn array<T>(
        &mut self,
        what: &'static str,
        stride: usize,
        mut element: impl FnMut(&mut Self) -> Result<T, DagDecodeError>,
    ) -> Result<Vec<T>, DagDecodeError> {
        let count = self.count(what, stride)?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(element(self)?);
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::meshlet::ClusterBounds;

    /// An edge named by where its endpoints *are* rather than by which vertex
    /// they were, so it is the same edge on either side of a level change.
    ///
    /// Two levels have unrelated vertex numbering and unrelated index buffers,
    /// so an index pair cannot say whether they meet. A bit pattern can, and
    /// exactly: a vertex the decimator never moved comes through the level
    /// change unchanged, so the coarser level's copy of a locked boundary vertex
    /// is bit-identical to the finer level's. Comparing bits rather than
    /// distances is what makes "they meet" mean *meet*, with no tolerance to
    /// tune.
    type SharedEdge = [[u32; 3]; 2];

    fn shared_edge(positions: &[[f32; 3]], a: u32, b: u32) -> SharedEdge {
        let mut edge = [positions[a as usize], positions[b as usize]].map(|p| p.map(f32::to_bits));
        edge.sort_unstable();
        edge
    }

    /// One cluster's triangles, as indices into its level's positions.
    fn cluster_triangles(clusters: &MeshClusters, cluster: usize) -> Vec<[u32; 3]> {
        let cluster = &clusters.clusters[cluster];
        let run =
            &clusters.vertices[cluster.vertex_offset as usize..][..cluster.vertex_count as usize];
        clusters.corners[cluster.triangle_offset as usize..][..cluster.triangle_count as usize * 3]
            .chunks_exact(3)
            .map(|face| [0, 1, 2].map(|corner| run[usize::from(face[corner])]))
            .collect()
    }

    /// The clusters an "is this group expanded?" answer draws.
    ///
    /// `docs/plan/25-lod.md`'s descent, and the whole of it: a cluster is drawn
    /// when the group that *produced* it is not expanded and the group that
    /// *contains* it is. Both halves ask `expanded` about a **group**, which is
    /// why one answer per group is all the rule needs and why every cluster a
    /// group produced moves together.
    ///
    /// A cluster no group contains defaults to **not** drawn, which is only
    /// correct on the top level. That is deliberate: a level whose grouping
    /// missed a cluster leaves a hole the cover check reports by name, where a
    /// default of "drawn" would quietly produce an overlap instead.
    fn descend(dag: &ClusterDag, expanded: impl Fn(&ClusterGroup) -> bool) -> Vec<(usize, usize)> {
        let top = dag.levels.len() - 1;
        let mut drawn = Vec::new();
        for (level, here) in dag.levels.iter().enumerate() {
            let count = here.clusters.clusters.len();
            let mut container = vec![level == top; count];
            for group in &here.groups {
                let open = expanded(group);
                for &child in &group.children {
                    container[child as usize] = open;
                }
            }

            let mut producer = vec![false; count];
            if level > 0 {
                for group in &dag.levels[level - 1].groups {
                    let open = expanded(group);
                    for &parent in &group.parents {
                        producer[parent as usize] = open;
                    }
                }
            }

            for cluster in 0..count {
                if !producer[cluster] && container[cluster] {
                    drawn.push((level, cluster));
                }
            }
        }
        drawn
    }

    /// The clusters a camera at `eye` draws — the rule an amplification stage
    /// will run, evaluated host-side.
    fn camera_cut(dag: &ClusterDag, eye: [f32; 3], budget: f32) -> Vec<(usize, usize)> {
        descend(dag, |group| {
            group.projected_error(eye, PIXELS_PER_UNIT) > budget
        })
    }

    /// How many pixels one unit of length subtends one unit from the eye, for
    /// the frames this workspace's goldens are drawn at: a 192-pixel-high
    /// viewport at a 60-degree vertical field of view.
    ///
    /// Written as a literal rather than `0.5 * 192.0 / (PI / 6.0).tan()` because
    /// `tanf` is not correctly rounded and differs in the last place between
    /// libms — the same argument [`crate::dunes`] makes about `sinf` — and the
    /// counts below are pinned by equality.
    const PIXELS_PER_UNIT: f32 = 166.0;

    /// Every threshold at which a cut changes, plus one past the last, so a
    /// sweep visits every distinct cut a global error budget admits.
    fn every_threshold(dag: &ClusterDag) -> Vec<f32> {
        let mut errors: Vec<f32> = dag
            .levels
            .iter()
            .flat_map(|level| level.errors.iter().copied())
            .collect();
        errors.sort_by(f32::total_cmp);
        errors.dedup();
        let last = *errors.last().expect("level 0 has clusters");
        errors
            .windows(2)
            .map(|pair| 0.5 * (pair[0] + pair[1]))
            .chain([last + 1.0])
            .collect()
    }

    /// Every edge of a cut's triangles, by position, and the levels of the
    /// clusters that carry it.
    fn cut_edges(dag: &ClusterDag, drawn: &[(usize, usize)]) -> BTreeMap<SharedEdge, Vec<usize>> {
        let mut edges: BTreeMap<SharedEdge, Vec<usize>> = BTreeMap::new();
        for &(level, cluster) in drawn {
            let positions = &dag.levels[level].positions;
            for face in cluster_triangles(&dag.levels[level].clusters, cluster) {
                for corner in 0..3 {
                    let edge = shared_edge(positions, face[corner], face[(corner + 1) % 3]);
                    edges.entry(edge).or_default().push(level);
                }
            }
        }
        edges
    }

    /// The base mesh's own boundary loop, by position: the edges a cut is
    /// *supposed* to leave with one face. The dunes patch is an open sheet, so
    /// this is its outer ring.
    fn base_border(positions: &[[f32; 3]], indices: &[u32]) -> BTreeSet<SharedEdge> {
        let mut uses: BTreeMap<SharedEdge, usize> = BTreeMap::new();
        for face in indices.chunks_exact(3) {
            for corner in 0..3 {
                *uses
                    .entry(shared_edge(positions, face[corner], face[(corner + 1) % 3]))
                    .or_default() += 1;
            }
        }
        uses.into_iter()
            .filter(|&(_, uses)| uses != 2)
            .map(|(edge, _)| edge)
            .collect()
    }

    /// Asserts a cut is a crack-free cover of the surface, and returns how many
    /// of its edges have a different level on either side.
    ///
    /// One check does both halves, and it is the strongest form of either.
    /// Every edge of the drawn triangles, keyed by the *positions* of its
    /// endpoints, must be used exactly twice — except the base mesh's own
    /// border, used once. An edge used once anywhere else is a hole: two
    /// clusters that should have met did not. An edge used three times or more
    /// is an overlap. And the two sides of an interface edge only land on the
    /// same key at all if the coarser level kept the finer level's vertices
    /// exactly, which is what makes this a test of the artifact and not only of
    /// its index arithmetic.
    fn assert_cut_is_a_crack_free_cover(
        dag: &ClusterDag,
        drawn: &[(usize, usize)],
        border: &BTreeSet<SharedEdge>,
    ) -> usize {
        let edges = cut_edges(dag, drawn);
        let single: BTreeSet<SharedEdge> = edges
            .iter()
            .filter(|&(_, levels)| levels.len() == 1)
            .map(|(&edge, _)| edge)
            .collect();
        // Reported as a count and a sample rather than as two sets: the border
        // of this patch is hundreds of edges and printing both sides of the
        // comparison buries the handful that actually differ.
        let holes: Vec<&SharedEdge> = single.difference(border).take(3).collect();
        let missing: Vec<&SharedEdge> = border.difference(&single).take(3).collect();
        assert!(
            holes.is_empty() && missing.is_empty(),
            "the cut's one-sided edges are not the base mesh's border, so it has a hole \
             or does not cover the whole surface: {} edges have one face where the mesh \
             is closed (first {holes:?}) and {} of the mesh's border edges have two or \
             none (first {missing:?})",
            single.difference(border).count(),
            border.difference(&single).count()
        );
        assert!(
            edges.values().all(|levels| levels.len() <= 2),
            "an edge with three faces on it is two clusters overlapping"
        );
        edges
            .values()
            .filter(|levels| levels.len() == 2 && levels[0] != levels[1])
            .count()
    }

    /// The committed artifact is exactly what this codec writes: decoding it and
    /// encoding the result reproduces the file byte for byte.
    ///
    /// This is the half of the arrangement that runs with no `crcbl-scene` in
    /// sight. `tools/cook-clusters.rs --check` is the other half and the one
    /// that can tell whether the *builder* still produces it; this one catches a
    /// decoder that reads the file into something else and hands it on.
    #[test]
    fn the_committed_artifact_round_trips_through_the_codec() {
        let dag = dunes_dag();
        assert!(
            dag.levels.len() > 3,
            "a DAG of {} levels proves little",
            dag.levels.len()
        );
        assert_eq!(
            dag.to_bytes(),
            include_bytes!("../clusters/dunes.dag"),
            "the committed bytes are not what this codec writes for what it read"
        );
        assert_eq!(
            dag.levels[0].positions,
            dunes::positions(),
            "level 0 is the model"
        );
    }

    /// Every level's clusters describe triangles over that level's own
    /// positions, and level 0's are the model's own triangles.
    ///
    /// An offset short by one cluster, a corner run that restarted in the wrong
    /// place, or a run length read off the wrong field each survive a
    /// byte-comparison against a file written with the same mistake, and none
    /// of them survive this.
    #[test]
    fn every_level_decodes_to_triangles_over_its_own_positions() {
        let dag = dunes_dag();
        for (depth, level) in dag.levels.iter().enumerate() {
            let mut triangles = 0usize;
            for cluster in 0..level.clusters.clusters.len() {
                for face in cluster_triangles(&level.clusters, cluster) {
                    assert!(
                        face.iter()
                            .all(|&index| (index as usize) < level.positions.len()),
                        "level {depth} cluster {cluster} names a vertex the level does not have"
                    );
                    assert!(
                        face[0] != face[1] && face[1] != face[2] && face[0] != face[2],
                        "level {depth} cluster {cluster} has a degenerate triangle {face:?}"
                    );
                    triangles += 1;
                }
            }
            assert_eq!(
                triangles,
                level
                    .clusters
                    .clusters
                    .iter()
                    .map(|cluster| cluster.triangle_count as usize)
                    .sum::<usize>(),
                "level {depth} decoded a different number of triangles than its clusters claim"
            );
            assert_eq!(level.errors.len(), level.clusters.clusters.len());
            assert_eq!(level.bounds.len(), level.clusters.clusters.len());
        }

        // Level 0 is the model's own triangles, permuted into cluster order.
        let mut decoded: Vec<[u32; 3]> = (0..dag.levels[0].clusters.clusters.len())
            .flat_map(|cluster| cluster_triangles(&dag.levels[0].clusters, cluster))
            .collect();
        let mut expected: Vec<[u32; 3]> = dunes::indices()
            .chunks_exact(3)
            .map(|face| [face[0], face[1], face[2]])
            .collect();
        decoded.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            decoded, expected,
            "level 0 is not the dunes patch's triangles"
        );
    }

    /// **The DAG gets coarser at every level**, which every other invariant here
    /// is satisfied by a builder that never simplified at all.
    #[test]
    fn the_dag_gets_coarser_at_every_level() {
        let dag = dunes_dag();
        let triangles: Vec<u32> = dag
            .levels
            .iter()
            .map(|level| {
                level
                    .clusters
                    .clusters
                    .iter()
                    .map(|cluster| cluster.triangle_count)
                    .sum()
            })
            .collect();
        let clusters: Vec<usize> = dag
            .levels
            .iter()
            .map(|level| level.clusters.clusters.len())
            .collect();
        for pair in triangles.windows(2) {
            assert!(pair[1] < pair[0], "triangles {triangles:?} do not shrink");
        }
        for pair in clusters.windows(2) {
            assert!(pair[1] < pair[0], "clusters {clusters:?} do not shrink");
        }
    }

    /// **Error never decreases up the DAG, and every cluster a group produced
    /// carries that group's number.**
    ///
    /// Monotonic error is what makes a cut well defined: along the groups
    /// covering any one region the errors never fall, so the descent's test
    /// holds at exactly one of them. Lose it and a cluster can be drawn while an
    /// ancestor covering it is drawn too, which is an overlap rather than a cut.
    #[test]
    fn the_error_never_decreases_up_the_dag() {
        let dag = dunes_dag();
        assert!(
            dag.levels[0].errors.iter().all(|&error| error == 0.0),
            "level 0 is the base mesh untouched and must read zero"
        );
        let mut groups = 0usize;
        for (depth, level) in dag.levels.iter().enumerate() {
            for (index, group) in level.groups.iter().enumerate() {
                assert!(
                    !group.children.is_empty(),
                    "level {depth} group {index} is empty"
                );
                assert!(
                    !group.parents.is_empty(),
                    "level {depth} group {index} produced nothing"
                );
                for &child in &group.children {
                    assert!(
                        group.error >= level.errors[child as usize],
                        "level {depth} group {index} costs {} and its child {child} already \
                         cost {}",
                        group.error,
                        level.errors[child as usize]
                    );
                }
                for &parent in &group.parents {
                    assert_eq!(
                        dag.levels[depth + 1].errors[parent as usize],
                        group.error,
                        "level {depth} group {index}'s parent {parent} carries an error \
                         that is not the group's"
                    );
                    assert_eq!(
                        dag.levels[depth + 1].bounds[parent as usize],
                        group.bounds,
                        "level {depth} group {index}'s parent {parent} carries a sphere \
                         that is not the group's"
                    );
                }
                groups += 1;
            }
        }
        assert!(groups > 0, "no group was checked, so nothing was");
    }

    /// **A group's sphere contains every sphere below it**, which is what
    /// carries monotone stored error into monotone *projected* error.
    ///
    /// A containing sphere is never further from any eye than one inside it, so
    /// `error / distance` rises up the DAG for every eye there is and the
    /// descent has one stopping point per branch whatever the camera does.
    /// Without it a closer group can project larger from a smaller error, and
    /// the cut stops being a cut.
    #[test]
    fn a_group_sphere_contains_every_sphere_below_it() {
        let dag = dunes_dag();
        let mut checked = 0usize;
        for (depth, level) in dag.levels.iter().enumerate() {
            for (index, group) in level.groups.iter().enumerate() {
                for &child in &group.children {
                    assert!(
                        group.bounds.contains(level.bounds[child as usize]),
                        "level {depth} group {index}'s sphere {:?} does not contain its \
                         child {child}'s {:?}",
                        group.bounds,
                        level.bounds[child as usize]
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no containment was checked");
    }

    /// **Every cut a global error budget admits is a crack-free cover of the
    /// surface**, over a sweep that visits each distinct one.
    #[test]
    fn every_error_budget_draws_a_crack_free_cut() {
        let dag = dunes_dag();
        let border = base_border(&dunes::positions(), &dunes::indices());
        let thresholds = every_threshold(&dag);
        assert!(
            thresholds.len() > 3,
            "a sweep of {thresholds:?} visits too few cuts"
        );

        let mut mixed = 0usize;
        for threshold in &thresholds {
            let drawn = descend(&dag, |group| *threshold < group.error);
            let interfaces = assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
            let levels: BTreeSet<usize> = drawn.iter().map(|&(level, _)| level).collect();
            if levels.len() > 1 {
                mixed += 1;
                assert!(
                    interfaces > 0,
                    "a cut spanning {levels:?} whose levels never meet is not a mixed cut, \
                     it is two meshes"
                );
            }
        }
        assert!(
            mixed > 0,
            "no error budget drew more than one level, so the sweep never exercised an \
             interface between two"
        );
    }

    /// **Every camera position draws a crack-free cut**, and most of them draw
    /// more than one level.
    ///
    /// This is the projected-error rule an amplification stage will run, which
    /// is where a per-cluster distance term would tear a group in half. Running
    /// it over the cooked artifact rather than over an in-memory DAG is the
    /// point: it is the committed bytes that reach the renderer.
    #[test]
    fn the_cooked_dag_draws_a_crack_free_cut_from_every_camera() {
        let dag = dunes_dag();
        let border = base_border(&dunes::positions(), &dunes::indices());

        let (mut mixed, mut cuts) = (0usize, 0usize);
        for eye in EYES {
            for budget in BUDGETS {
                let drawn = camera_cut(&dag, eye, budget);
                let interfaces = assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
                let levels: BTreeSet<usize> = drawn.iter().map(|&(level, _)| level).collect();
                if levels.len() > 1 {
                    mixed += 1;
                    assert!(
                        interfaces > 0,
                        "a cut spanning {levels:?} from {eye:?} whose levels never meet is \
                         not a mixed cut, it is two meshes"
                    );
                }
                cuts += 1;
            }
        }
        assert_eq!(
            cuts,
            EYES.len() * BUDGETS.len(),
            "the sweep has to have run"
        );
        assert!(
            mixed > cuts / 2,
            "only {mixed} of {cuts} camera cuts held more than one level, so this is \
             mostly re-testing uniform cuts under a new name"
        );
    }

    /// **The thing per-cluster selection is for**, in numbers: one draw of one
    /// mesh whose near end is at a finer level than its far end.
    ///
    /// The distance term is what produces it. A level's stored errors barely
    /// vary within the level — the decimator targets a triangle count, so a
    /// level's error frontier is flat — and a cut reading them alone would be
    /// uniform at every camera. Split the drawn clusters by how far up the patch
    /// they sit and the two ends report different levels.
    ///
    /// The histograms are pinned rather than compared loosely, because "the near
    /// end is finer" is satisfied by a cut one cluster away from uniform, and
    /// that is not the claim.
    #[test]
    fn a_receding_patch_draws_its_near_end_finer_than_its_far_end() {
        let dag = dunes_dag();
        let eye = EYES[0];
        let drawn = camera_cut(&dag, eye, MIXING_BUDGET);

        let mut near: BTreeMap<usize, usize> = BTreeMap::new();
        let mut far: BTreeMap<usize, usize> = BTreeMap::new();
        for &(level, cluster) in &drawn {
            let depth = dag.levels[level].clusters.clusters[cluster].bounds.center[2];
            if depth < NEAR_THIRD {
                *near.entry(level).or_default() += 1;
            } else if depth > FAR_THIRD {
                *far.entry(level).or_default() += 1;
            }
        }
        assert_eq!(
            near,
            NEAR_HISTOGRAM.iter().copied().collect::<BTreeMap<_, _>>(),
            "the near third of the patch is not the levels it was cooked to be"
        );
        assert_eq!(
            far,
            FAR_HISTOGRAM.iter().copied().collect::<BTreeMap<_, _>>(),
            "the far third of the patch is not the levels it was cooked to be"
        );
        assert!(
            near.keys().min() < far.keys().min(),
            "the near end {near:?} is not drawn finer than the far end {far:?}"
        );

        // And the two ends are one cut of one mesh, not two pictures.
        let border = base_border(&dunes::positions(), &dunes::indices());
        let interfaces = assert_cut_is_a_crack_free_cover(&dag, &drawn, &border);
        assert!(
            interfaces > 20,
            "only {interfaces} edges have one level on one side and another on the other, \
             which is too few to be the seam between the two ends"
        );
    }

    /// Pixel budgets the patch actually changes its cut across.
    ///
    /// Below the smallest of these every camera here draws level 0 whole and
    /// above the largest the DAG has run out of levels, so a sweep outside the
    /// band would be a sweep over one answer. The band is a property of this
    /// model's errors and its size, not a tuning the engine ships.
    const BUDGETS: [f32; 5] = [8.0, 16.0, 32.0, 64.0, 128.0];

    /// The budget the near/far histograms are read at: inside the band where
    /// the near end's groups and the far end's fall on opposite sides of the
    /// test at [`EYES`]`[0]`.
    const MIXING_BUDGET: f32 = 32.0;

    /// How many clusters of each level the near and far thirds of the patch draw
    /// at [`MIXING_BUDGET`] from [`EYES`]`[0]`.
    ///
    /// Read off a run of the committed artifact, and pinned: this is the
    /// observable the whole slice exists to produce, so it is the one number
    /// here that has no derivation to check it against — a change in it is a
    /// change in what the model shows, and has to be looked at rather than
    /// absorbed.
    const NEAR_HISTOGRAM: &[(usize, usize)] = &[(0, 13), (1, 12)];
    const FAR_HISTOGRAM: &[(usize, usize)] = &[(2, 15)];

    /// Where the near third of the patch ends and the far third begins, along
    /// the axis running away from [`EYES`]`[0]`. The middle third is left out of
    /// both histograms: it is where the two ends change over, and which level a
    /// cluster there takes is the thing the budget decides rather than the thing
    /// being asserted.
    const NEAR_THIRD: f32 = -dunes::DUNES_EXTENT / 3.0;
    const FAR_THIRD: f32 = dunes::DUNES_EXTENT / 3.0;

    /// Where the sweep puts the camera, in the model's own space.
    ///
    /// `dunes` lays the patch out centred on the origin in `x` and `z` with the
    /// height on `y`, so an eye at negative `z` and a small `y` is a viewer
    /// standing at one edge of a ground plane that recedes away from them — the
    /// shape `docs/plan/25-lod.md`'s per-cluster selection exists for. The rest
    /// are off a corner, high above, and level with the middle, so the sweep is
    /// not one camera's arrangement holding.
    const EYES: [[f32; 3]; 5] = [
        [0.0, 4.0, -dunes::DUNES_EXTENT - 2.0],
        [0.0, 8.0, -dunes::DUNES_EXTENT - 24.0],
        [
            -dunes::DUNES_EXTENT - 16.0,
            6.0,
            -dunes::DUNES_EXTENT - 16.0,
        ],
        [0.0, 4.0 * dunes::DUNES_EXTENT, 0.0],
        [0.0, 3.0, 0.0],
    ];

    /// A decoder that reads past the end of its input, or reads a file that is
    /// not one of these, refuses by name rather than producing a plausible DAG.
    #[test]
    fn a_corrupt_artifact_is_refused_rather_than_decoded() {
        let good = dunes_dag().to_bytes();
        let base = dunes::positions();

        assert!(
            ClusterDag::from_bytes(&good, base.clone()).is_ok(),
            "the artifact this test perturbs has to decode first"
        );

        let mut wrong_magic = good.clone();
        wrong_magic[0] ^= 0xff;
        assert_eq!(
            ClusterDag::from_bytes(&wrong_magic, base.clone()),
            Err(DagDecodeError {
                what: "the magic is not a cooked cluster DAG",
                at: 0
            })
        );

        let mut wrong_version = good.clone();
        wrong_version[MAGIC.len()] = wrong_version[MAGIC.len()].wrapping_add(1);
        assert_eq!(
            ClusterDag::from_bytes(&wrong_version, base.clone()),
            Err(DagDecodeError {
                what: "the version is not the one this decoder writes",
                at: MAGIC.len()
            })
        );

        let mut trailing = good.clone();
        trailing.push(0);
        assert_eq!(
            ClusterDag::from_bytes(&trailing, base.clone()),
            Err(DagDecodeError {
                what: "there are bytes after the last level",
                at: good.len()
            })
        );

        // A truncation at every length, so the walk is shown to run out of bytes
        // rather than to read whatever is next in memory.
        for length in [
            0,
            4,
            MAGIC.len(),
            MAGIC.len() + 4,
            good.len() / 2,
            good.len() - 1,
        ] {
            assert!(
                ClusterDag::from_bytes(&good[..length], base.clone()).is_err(),
                "{length} bytes of the artifact decoded as a whole DAG"
            );
        }

        // A count that would address more than the file holds is refused before
        // anything is allocated on it.
        let mut huge_level_count = good.clone();
        huge_level_count[MAGIC.len() + 4..MAGIC.len() + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            ClusterDag::from_bytes(&huge_level_count, base).map(|dag| dag.levels.len()),
            Err(DagDecodeError {
                what: "the level count",
                at: MAGIC.len() + 8
            })
        );
    }

    /// **A `base_positions` that is not the array the artifact was cooked over is
    /// refused**, rather than decoding into a DAG that panics in whoever walks
    /// it.
    ///
    /// Level 0's positions come from the caller and not from the file, which is
    /// what buys the artifact its largest array back — and is the one way a
    /// well-formed file still decodes into something wrong. Nothing in the file
    /// can catch it; only the index ranges can.
    #[test]
    fn positions_the_artifact_was_not_cooked_over_are_refused() {
        let good = dunes_dag().to_bytes();
        let mut short = dunes::positions();
        short.truncate(short.len() - 1);
        assert_eq!(
            ClusterDag::from_bytes(&good, short)
                .err()
                .map(|error| error.what),
            Some("a cluster names a vertex the level does not have"),
        );

        // A longer array is not refused — every index still names a vertex — so
        // the check above is about reachability rather than about equality, and
        // says so rather than pretending to a check it does not make.
        let mut long = dunes::positions();
        long.push([0.0; 3]);
        assert!(ClusterDag::from_bytes(&good, long).is_ok());
    }

    /// A cluster whose runs reach past the arrays they index is refused by name,
    /// on each of the three ranges that can go wrong.
    ///
    /// Reached by building the levels directly rather than by perturbing the
    /// artifact: a byte flip lands wherever it lands, and every one of these has
    /// to be shown to fire.
    #[test]
    fn a_cluster_reaching_past_its_arrays_is_refused_by_name() {
        let sound = |clusters: Vec<Meshlet>, vertices: Vec<u32>, corners: Vec<u8>| DagLevel {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            errors: vec![0.0; clusters.len()],
            bounds: vec![
                GroupBounds {
                    center: [0.0; 3],
                    radius: 1.0
                };
                clusters.len()
            ],
            clusters: MeshClusters {
                clusters,
                vertices,
                corners,
            },
            groups: Vec::new(),
        };
        let cluster = |vertex_count, triangle_count| {
            Meshlet::new(0, vertex_count, 0, triangle_count, ClusterBounds::default())
                .expect("small numbers")
        };

        // The shape everything below is a single change away from, so the
        // refusals are about the change rather than about the fixture.
        let whole = sound(vec![cluster(3, 1)], vec![0, 1, 2], vec![0, 1, 2]);
        assert_eq!(check_indices_are_in_range(&whole, 0), Ok(()));

        for (level, expected) in [
            (
                sound(vec![cluster(4, 1)], vec![0, 1, 2], vec![0, 1, 2]),
                "a cluster's vertex run is not inside the vertex array",
            ),
            (
                sound(vec![cluster(3, 2)], vec![0, 1, 2], vec![0, 1, 2]),
                "a cluster's corners are not inside the corner array",
            ),
            (
                sound(vec![cluster(3, 1)], vec![0, 1, 9], vec![0, 1, 2]),
                "a cluster names a vertex the level does not have",
            ),
            (
                sound(vec![cluster(3, 1)], vec![0, 1, 2], vec![0, 1, 3]),
                "a corner reaches past its own cluster's vertex run",
            ),
        ] {
            assert_eq!(
                check_indices_are_in_range(&level, 7),
                Err(DagDecodeError {
                    what: expected,
                    at: 7
                })
            );
        }
    }

    /// The error message names what ran out and where, because a corrupt bake
    /// artifact is something someone has to act on.
    #[test]
    fn the_decode_error_names_what_and_where() {
        let message = DagDecodeError {
            what: "a cluster sphere",
            at: 4096,
        }
        .to_string();
        assert!(message.contains("a cluster sphere"), "{message}");
        assert!(message.contains("4096"), "{message}");
    }
}
