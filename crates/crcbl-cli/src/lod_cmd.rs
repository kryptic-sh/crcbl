//! `crcbl lod` — what a mesh's levels are, and where they come from.
//!
//! `docs/plan/25-lod.md`'s Tooling row asks for `crcbl lod gen|stats|preview`.
//! Two of the three are here; `preview` is not, and says so rather than being
//! absent (see [`LodAction::Preview`]).
//!
//! # `stats` is where "no silent substitution" becomes checkable
//!
//! The plan states that a level is either the artist's geometry or the
//! generator's, never one standing in for the other, and that "the import report
//! says which levels came from where". [`crcbl_scene::resolve_lod`] is what
//! decides it and [`LodOrigin`] is what records it; this command is the place a
//! person reads it without writing a test. So provenance is a column of the
//! table rather than a footnote, and the two kinds of level deliberately do
//! **not** report the same fields:
//!
//! * A **generated** level has a cluster count and an error range, because the
//!   engine built it and measured it.
//! * A **hand-authored** level below LOD0 has neither. It was never clustered
//!   and never decimated here — that is the whole point of it — so there is no
//!   engine number to print, and printing one would be inventing it. The human
//!   table shows `-` and `--json` omits the key.
//! * **LOD0 is both**: hand-authored, since it is the file's own geometry, and
//!   clustered whenever the generator ran, since the generator clustered it as
//!   DAG level 0 at an error of zero. It reports those.
//!
//! # A refusal is an error, not a row
//!
//! Everything [`crcbl_scene::LodResolveError`] refuses — two nodes claiming one
//! level, an `MSFT_lod` entry drawing nothing, a gap the generator cannot reach
//! — fails the command, naming the node and the reason. A table printed beside a
//! refusal is worse than no table: it looks like an answer. The same applies to
//! a mesh the importer emptied, which is the one way a chain can be *resolvable*
//! and still describe nothing — see [`check_has_triangles`].

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crcbl_assets::DirSource;
use crcbl_scene::{ClusterDag, GltfScene, LodOrigin, MeshLod, import_gltf, resolve_lod};
use crcbl_store::web::canonical_key;

use crate::args::{LodAction, LodArgs};
use crate::json::Json;
use crate::report::{Failure, Outcome};

/// The share of a level's triangles surviving into the level above that counts
/// as a stall.
///
/// [`crcbl_scene::build_cluster_dag`] asks each level for half the triangles of
/// the one below, and the locked group boundaries are what can stop it getting
/// there. A level that kept more than this got less than half of the reduction
/// it asked for, which is a finding worth a person's attention rather than a
/// number to skim past: this workspace has already hit the case where a group
/// whose edges were all locked carried its children's error up unchanged.
///
/// It is a reporting threshold and nothing selects on it.
const STALL_KEPT: f32 = 0.75;

/// Runs `crcbl lod`.
///
/// # Errors
///
/// [`Failure`] if the file cannot be imported, if the selection names nothing,
/// if a chain cannot be resolved, or — for `gen` — if there is no DAG to write
/// or the output is occupied.
pub fn run(args: &LodArgs) -> Result<Outcome, Failure> {
    if args.action == LodAction::Preview {
        // Refused before the file is read: the answer does not depend on it,
        // and a refusal that first spent a DAG build would be a lie about what
        // the command tried.
        return Err(Failure::new(
            "`crcbl lod preview` is not implemented. `stats` and `gen` are host-only; a \
             preview renders each level offscreen and needs the GPU path `crcbl screenshot` \
             has — see `docs/plan/25-lod.md`",
        )
        .with("action", Json::string(LodAction::Preview.name())));
    }

    let scene = open(&args.file)?;
    match args.action {
        LodAction::Stats => stats(&scene, args),
        LodAction::Gen => generate(&scene, args),
        LodAction::Preview => unreachable!("refused above"),
    }
}

/// Imports the glTF the command line named.
///
/// The importer reads through [`crcbl_assets::AssetSource`], which addresses a
/// *key under a root* rather than a path, so a path typed on a command line is
/// split into the two: the directory becomes the root, the file name becomes the
/// key. The key rules are the engine's own and are checked here rather than left
/// to surface as `path escapes the storage root`, which is true and says nothing
/// about the name that was actually the problem.
fn open(file: &Path) -> Result<GltfScene, Failure> {
    let Some(name) = file.file_name() else {
        return Err(Failure::new(format!(
            "{} does not name a file to read",
            file.display()
        )));
    };
    let key = canonical_key(Path::new(name)).map_err(|_| {
        Failure::new(format!(
            "`{}` is not a usable asset name: a key is ASCII letters, digits, `.`, `_` and \
             `-`, because the same name has to resolve as a URL path in a browser",
            name.to_string_lossy()
        ))
    })?;
    let root = match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        // `crcbl lod stats model.glb`: the root is where the shell was run.
        _ => PathBuf::from("."),
    };

    import_gltf(&DirSource::at(root), Path::new(&key))
        .map_err(|error| Failure::new(format!("cannot import {}: {error}", file.display())))
}

/// ``<index> `<name>` `` for a node, or just the index when the file named
/// none.
fn node_label(scene: &GltfScene, node: usize) -> String {
    match scene
        .nodes()
        .get(node)
        .and_then(crcbl_scene::GltfNode::name)
    {
        Some(name) => format!("{node} `{name}`"),
        None => node.to_string(),
    }
}

/// The same for a mesh.
fn mesh_label(scene: &GltfScene, mesh: usize) -> String {
    match scene
        .meshes()
        .get(mesh)
        .and_then(crcbl_scene::GltfMesh::name)
    {
        Some(name) => format!("{mesh} `{name}`"),
        None => mesh.to_string(),
    }
}

/// Resolves one node's chain, with the node named in whatever it refuses.
fn resolve(scene: &GltfScene, node: usize) -> Result<MeshLod, Failure> {
    resolve_lod(scene, node).map_err(|error| {
        Failure::new(format!(
            "node {} has no LOD chain: {error}",
            node_label(scene, node)
        ))
        .with("node", Json::Number(node as i64))
    })
}

/// Refuses a mesh with no triangles to have levels of.
///
/// The importer skips a primitive that is not a triangle list — a point cloud, a
/// line set — and says so through `log::warn!`, which a CLI that installs no
/// logger never shows. A mesh whose every primitive went that way still resolves:
/// it has no DAG to build, so the chain is LOD0 alone, and the table would read
/// as a one-level mesh of nothing rather than as the import problem it is.
fn check_has_triangles(scene: &GltfScene, node: usize, mesh: usize) -> Result<(), Failure> {
    if scene.meshes()[mesh].primitives().is_empty() {
        return Err(Failure::new(format!(
            "node {} draws mesh {}, which has no triangles: every primitive of it was \
             skipped at import because it is not a triangle list, so there is nothing to \
             have levels of",
            node_label(scene, node),
            mesh_label(scene, mesh)
        ))
        .with("node", Json::Number(node as i64))
        .with("mesh", Json::Number(mesh as i64)));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `crcbl lod stats`
// ---------------------------------------------------------------------------

/// One resolved chain, in the numbers the report prints.
struct Chain {
    node: usize,
    mesh: usize,
    levels: Vec<Level>,
    dags: Vec<Dag>,
    /// The other nodes this chain claims as hand-authored levels, so they are
    /// not also reported as chains of their own.
    claims: Vec<usize>,
}

/// One level of a chain.
struct Level {
    origin: LodOrigin,
    triangles: usize,
    /// `None` on a hand-authored level: nothing clustered it. See the module
    /// docs.
    clusters: Option<usize>,
    /// `None` for the same reason, and on a level with no clusters to fold.
    error: Option<(f32, f32)>,
}

/// One primitive's DAG, level by level.
struct Dag {
    primitive: usize,
    levels: Vec<DagLevel>,
}

/// One level of a DAG.
struct DagLevel {
    triangles: usize,
    clusters: usize,
    groups: usize,
    error: Option<(f32, f32)>,
    /// This level's triangles as a share of the level below's. `None` on level
    /// 0, which is below nothing.
    kept: Option<f32>,
}

impl DagLevel {
    /// Whether the decimator got well short of the halving it asked for.
    fn stalled(&self) -> bool {
        self.kept.is_some_and(|kept| kept > STALL_KEPT)
    }
}

fn stats(scene: &GltfScene, args: &LodArgs) -> Result<Outcome, Failure> {
    let mut chains: Vec<Chain> = Vec::new();
    let mut claimed: Vec<usize> = Vec::new();

    for node in selection(scene, args) {
        // A node an earlier chain already reported as one of its levels is not
        // a chain of its own. Nothing in either convention points upwards, so
        // `resolve_lod` would happily treat `car_LOD1` as a base and report a
        // second chain for geometry already accounted for. `--node` bypasses
        // this: it is an explicit request for that node.
        if args.node.is_none() && claimed.contains(&node) {
            continue;
        }
        let chain = chain_of(scene, node)?;
        claimed.extend(&chain.claims);
        chains.push(chain);
    }

    if chains.is_empty() {
        return Err(Failure::new(format!(
            "{} draws no mesh, so it has no LOD chain",
            args.file.display()
        )));
    }

    let human = chains
        .iter()
        .map(|chain| chain_text(scene, chain))
        .collect::<Vec<_>>()
        .join("\n");
    let stalls: usize = chains
        .iter()
        .flat_map(|chain| &chain.dags)
        .flat_map(|dag| &dag.levels)
        .filter(|level| level.stalled())
        .count();

    Ok(Outcome {
        human: format!(
            "{}: {} chain{}{}\n{human}",
            args.file.display(),
            chains.len(),
            if chains.len() == 1 { "" } else { "s" },
            if stalls == 0 {
                String::new()
            } else {
                format!(", {stalls} stalled DAG level{}", plural(stalls))
            }
        ),
        json: vec![
            ("action", Json::string(LodAction::Stats.name())),
            ("path", Json::string(args.file.display().to_string())),
            ("stalled_levels", Json::Number(stalls as i64)),
            (
                "chains",
                Json::Array(
                    chains
                        .iter()
                        .map(|chain| chain_json(scene, chain))
                        .collect(),
                ),
            ),
        ],
    })
}

/// The nodes `stats` reports on, in scene order and without repeats.
fn selection(scene: &GltfScene, args: &LodArgs) -> Vec<usize> {
    if let Some(node) = args.node {
        return vec![node];
    }
    let mut nodes: Vec<usize> = Vec::new();
    for instance in scene.instances() {
        if !nodes.contains(&instance.node()) {
            nodes.push(instance.node());
        }
    }
    nodes
}

/// Resolves `node` and measures every level of the chain it produced.
fn chain_of(scene: &GltfScene, node: usize) -> Result<Chain, Failure> {
    let lod = resolve(scene, node)?;
    let mut levels = Vec::with_capacity(lod.levels().len());
    let mut claims = Vec::new();
    let mut base = 0;

    for (depth, &origin) in lod.levels().iter().enumerate() {
        let level = match origin {
            LodOrigin::Hand {
                node: from, mesh, ..
            } => {
                check_has_triangles(scene, from, mesh)?;
                if depth == 0 {
                    base = mesh;
                } else {
                    claims.push(from);
                }
                // LOD0 is hand-authored by definition and is still the array
                // the generator was handed, so when a DAG exists its level 0
                // *is* this level clustered, at an error of zero. Reporting
                // that is not a substitution: it is the base mesh, measured.
                let clustered = depth == 0 && !lod.dags().is_empty();
                Level {
                    origin,
                    triangles: mesh_triangles(scene, mesh),
                    clusters: clustered.then(|| cut_clusters(lod.dags(), 0)),
                    error: clustered.then(|| cut_error(lod.dags(), 0)).flatten(),
                }
            }
            LodOrigin::Generated { dag_level } => Level {
                origin,
                triangles: cut_triangles(lod.dags(), dag_level),
                clusters: Some(cut_clusters(lod.dags(), dag_level)),
                error: cut_error(lod.dags(), dag_level),
            },
        };
        levels.push(level);
    }

    Ok(Chain {
        node,
        mesh: base,
        levels,
        dags: lod.dags().iter().enumerate().map(dag_of).collect(),
        claims,
    })
}

/// Every triangle of every primitive of one glTF mesh.
fn mesh_triangles(scene: &GltfScene, mesh: usize) -> usize {
    scene.meshes()[mesh]
        .primitives()
        .iter()
        .map(|primitive| primitive.indices().len() / 3)
        .sum()
}

/// The triangles a uniform cut at `depth` draws — one depth read in every
/// primitive's DAG, which is what a generated chain level *is*.
fn cut_triangles(dags: &[ClusterDag], depth: usize) -> usize {
    dags.iter()
        .map(|dag| dag.levels()[depth].indices().len() / 3)
        .sum()
}

fn cut_clusters(dags: &[ClusterDag], depth: usize) -> usize {
    dags.iter()
        .map(|dag| dag.levels()[depth].clusters().clusters().len())
        .sum()
}

/// The error range over every cluster that cut draws.
fn cut_error(dags: &[ClusterDag], depth: usize) -> Option<(f32, f32)> {
    range(
        dags.iter()
            .flat_map(|dag| dag.levels()[depth].errors())
            .copied(),
    )
}

/// One primitive's DAG measured level by level.
fn dag_of((primitive, dag): (usize, &ClusterDag)) -> Dag {
    let triangles: Vec<usize> = dag
        .levels()
        .iter()
        .map(|level| level.indices().len() / 3)
        .collect();

    Dag {
        primitive,
        levels: dag
            .levels()
            .iter()
            .enumerate()
            .map(|(depth, level)| DagLevel {
                triangles: triangles[depth],
                clusters: level.clusters().clusters().len(),
                groups: level.groups().len(),
                error: range(level.errors().iter().copied()),
                // A level below with no triangles at all divides by zero and
                // would report an infinite share; there is no reduction to
                // measure against nothing, so it is no ratio rather than one.
                kept: depth
                    .checked_sub(1)
                    .filter(|&below| triangles[below] > 0)
                    .map(|below| triangles[depth] as f32 / triangles[below] as f32),
            })
            .collect(),
    }
}

/// The smallest and largest of `values`, or `None` when there are none.
fn range(values: impl Iterator<Item = f32>) -> Option<(f32, f32)> {
    values.fold(None, |bounds, value| match bounds {
        None => Some((value, value)),
        Some((low, high)) => Some((low.min(value), high.max(value))),
    })
}

/// One chain as a person reads it.
fn chain_text(scene: &GltfScene, chain: &Chain) -> String {
    let mut text = format!(
        "node {} — mesh {}, {} level{}\n",
        node_label(scene, chain.node),
        mesh_label(scene, chain.mesh),
        chain.levels.len(),
        plural(chain.levels.len())
    );

    for (depth, level) in chain.levels.iter().enumerate() {
        let _ = writeln!(
            text,
            "  LOD{depth}  {:<9}  {:<34}  {:>8} tris  {:>5} clusters  error {}",
            origin_kind(level.origin),
            origin_source(scene, level.origin),
            level.triangles,
            level
                .clusters
                .map_or_else(|| "-".to_string(), |clusters| clusters.to_string()),
            error_text(level.error),
        );
    }

    for dag in &chain.dags {
        let _ = writeln!(
            text,
            "  DAG, primitive {} of {}: {} level{}",
            dag.primitive,
            chain.dags.len(),
            dag.levels.len(),
            plural(dag.levels.len())
        );
        for (depth, level) in dag.levels.iter().enumerate() {
            let _ = writeln!(
                text,
                "    level {depth}: {:>8} tris  {:>5} clusters  {:>5} groups  error {}{}",
                level.triangles,
                level.clusters,
                level.groups,
                error_text(level.error),
                match level.kept {
                    None => String::new(),
                    Some(kept) => format!(
                        ",  kept {:.0}%{}",
                        kept * 100.0,
                        if level.stalled() { " — STALLED" } else { "" }
                    ),
                }
            );
        }
    }

    text.trim_end().to_string()
}

/// `hand` or `generated` — the answer to "did the artist ship this".
fn origin_kind(origin: LodOrigin) -> &'static str {
    match origin {
        LodOrigin::Hand { .. } => "hand",
        LodOrigin::Generated { .. } => "generated",
    }
}

/// Where the level's geometry is: the node that holds it, or the DAG depth.
fn origin_source(scene: &GltfScene, origin: LodOrigin) -> String {
    match origin {
        LodOrigin::Hand { node, via, .. } => {
            format!("node {} ({})", node_label(scene, node), link(via))
        }
        LodOrigin::Generated { dag_level } => format!("DAG level {dag_level}"),
    }
}

/// What tied a hand-authored level to its chain, in one word.
fn link(via: crcbl_scene::HandLodLink) -> &'static str {
    use crcbl_scene::HandLodLink as Link;
    match via {
        Link::Base => "base",
        Link::NodeName => "name",
        Link::MsftLod => "MSFT_lod",
        Link::NodeNameAndMsftLod => "name+MSFT_lod",
    }
}

/// An error range for a person: `-` when there is none to print.
///
/// Two spellings rather than one format string, because a geometric error is in
/// the mesh's own units and those span whatever the artist modelled in: a fixed
/// four decimals reads `0.0000` for every level of a mesh built in metres, which
/// is a report that hides exactly what it was asked for. `--json` carries the
/// value itself either way.
fn error_text(range: Option<(f32, f32)>) -> String {
    fn one(value: f32) -> String {
        if value != 0.0 && value.abs() < 1e-3 {
            format!("{value:.2e}")
        } else {
            format!("{value:.4}")
        }
    }
    match range {
        None => "-".to_string(),
        Some((low, high)) => format!("{}..{}", one(low), one(high)),
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// One chain in the machine-readable form.
///
/// The same facts as the table, so that `crcbl bench` and the editor's LOD panel
/// read them from here rather than growing a second report — see
/// `docs/plan/40-profiling.md`. A key that is absent is absent for a reason the
/// module docs give: a name the file never supplied, or a number that exists
/// only for a level the engine built.
fn chain_json(scene: &GltfScene, chain: &Chain) -> Json {
    let mut fields = vec![("node", Json::Number(chain.node as i64))];
    if let Some(name) = scene.nodes()[chain.node].name() {
        fields.push(("node_name", Json::string(name)));
    }
    fields.push(("mesh", Json::Number(chain.mesh as i64)));
    if let Some(name) = scene.meshes()[chain.mesh].name() {
        fields.push(("mesh_name", Json::string(name)));
    }
    fields.push((
        "levels",
        Json::Array(
            chain
                .levels
                .iter()
                .enumerate()
                .map(|(depth, level)| level_json(scene, depth, level))
                .collect(),
        ),
    ));
    fields.push((
        "dags",
        Json::Array(chain.dags.iter().map(dag_json).collect()),
    ));
    Json::Object(fields)
}

fn level_json(scene: &GltfScene, depth: usize, level: &Level) -> Json {
    let mut fields = vec![
        ("level", Json::Number(depth as i64)),
        ("origin", Json::string(origin_kind(level.origin))),
    ];
    match level.origin {
        LodOrigin::Hand { node, mesh, via } => {
            fields.push(("via", Json::string(link(via))));
            fields.push(("node", Json::Number(node as i64)));
            if let Some(name) = scene.nodes()[node].name() {
                fields.push(("node_name", Json::string(name)));
            }
            fields.push(("mesh", Json::Number(mesh as i64)));
        }
        LodOrigin::Generated { dag_level } => {
            fields.push(("dag_level", Json::Number(dag_level as i64)));
        }
    }
    fields.push(("triangles", Json::Number(level.triangles as i64)));
    if let Some(clusters) = level.clusters {
        fields.push(("clusters", Json::Number(clusters as i64)));
    }
    push_error(&mut fields, level.error);
    Json::Object(fields)
}

fn dag_json(dag: &Dag) -> Json {
    Json::Object(vec![
        ("primitive", Json::Number(dag.primitive as i64)),
        (
            "levels",
            Json::Array(
                dag.levels
                    .iter()
                    .enumerate()
                    .map(|(depth, level)| {
                        let mut fields = vec![
                            ("level", Json::Number(depth as i64)),
                            ("triangles", Json::Number(level.triangles as i64)),
                            ("clusters", Json::Number(level.clusters as i64)),
                            ("groups", Json::Number(level.groups as i64)),
                        ];
                        push_error(&mut fields, level.error);
                        if let Some(kept) = level.kept {
                            fields.push(("kept", Json::Float(kept)));
                        }
                        fields.push(("stalled", Json::Bool(level.stalled())));
                        Json::Object(fields)
                    })
                    .collect(),
            ),
        ),
    ])
}

fn push_error(fields: &mut Vec<(&'static str, Json)>, error: Option<(f32, f32)>) {
    if let Some((low, high)) = error {
        fields.push(("error_min", Json::Float(low)));
        fields.push(("error_max", Json::Float(high)));
    }
}

// ---------------------------------------------------------------------------
// `crcbl lod gen`
// ---------------------------------------------------------------------------

/// Builds one primitive's DAG and writes the cooked artifact.
///
/// One file holds one DAG, which is what the format is: `to_bytes` writes a
/// level list and nothing that could name a second mesh. So the selection has to
/// come down to one primitive of one node, and an ambiguous file is **refused
/// with the candidates named** rather than resolved by picking the first — the
/// same rule the resolver applies to two nodes claiming one level.
fn generate(scene: &GltfScene, args: &LodArgs) -> Result<Outcome, Failure> {
    let output = args
        .output
        .as_ref()
        .ok_or_else(|| Failure::new("`lod gen` needs an output path (-o <FILE>)"))?;
    // Refused before the DAG is built. Checking after would spend the whole
    // build to reach the same answer, and checking after the write is not a
    // check at all.
    if output.exists() && !args.force {
        return Err(Failure::new(format!(
            "{} already exists; pass --force to overwrite it",
            output.display()
        )));
    }

    let node = match args.node {
        Some(node) => node,
        None => only_node(scene, args)?,
    };
    let lod = resolve(scene, node)?;
    let Some(LodOrigin::Hand { mesh, .. }) = lod.levels().first().copied() else {
        unreachable!("LOD0 is always the resolved node's own mesh");
    };
    check_has_triangles(scene, node, mesh)?;

    if lod.dags().is_empty() {
        return Err(Failure::new(format!(
            "node {} has a hand-authored level for every level of its chain, so the \
             generator never ran and there is no DAG to write",
            node_label(scene, node)
        ))
        .with("node", Json::Number(node as i64)));
    }

    let primitive = match args.primitive {
        Some(primitive) => primitive,
        None if lod.dags().len() == 1 => 0,
        None => {
            return Err(Failure::new(format!(
                "mesh {} has {} primitives and one .dag holds one of them; pass \
                 --primitive <0..{}>",
                mesh_label(scene, mesh),
                lod.dags().len(),
                lod.dags().len() - 1
            ))
            .with("primitives", Json::Number(lod.dags().len() as i64)));
        }
    };
    let dag = lod.dags().get(primitive).ok_or_else(|| {
        Failure::new(format!(
            "mesh {} has {} primitives, so there is no primitive {primitive}",
            mesh_label(scene, mesh),
            lod.dags().len()
        ))
    })?;

    let cooked = dag.cook();
    let bytes = cooked.to_bytes();

    // What was generated has to decode, and this is the only place it can be
    // checked: an artifact its own reader refuses shows up in whatever loads it
    // later, with nothing left to say which bake wrote it. Level 0's positions
    // are not in the file — they are the array the builder was handed — so they
    // are supplied here from the same DAG they were cooked from.
    let base = dag.levels()[0].positions().to_vec();
    if crcbl::shaders::cluster_dag::ClusterDag::from_bytes(&bytes, base).as_ref() != Ok(&cooked) {
        return Err(Failure::new(
            "the generated .dag does not read back as what was cooked, which is a bug in \
             `crcbl lod gen`",
        ));
    }

    std::fs::write(output, &bytes)
        .map_err(|error| Failure::new(format!("cannot write {}: {error}", output.display())))?;

    let measured = dag_of((primitive, dag));
    let triangles = measured.levels.first().map_or(0, |level| level.triangles);
    let mut human = format!(
        "wrote {} ({} bytes, {} level{}, {triangles} base triangles)\n  node {}, mesh {}, \
         primitive {primitive}",
        output.display(),
        bytes.len(),
        measured.levels.len(),
        plural(measured.levels.len()),
        node_label(scene, node),
        mesh_label(scene, mesh),
    );
    for (depth, level) in measured.levels.iter().enumerate() {
        let _ = write!(
            human,
            "\n    level {depth}: {:>8} tris  {:>5} clusters  {:>5} groups  error {}",
            level.triangles,
            level.clusters,
            level.groups,
            error_text(level.error),
        );
    }

    Ok(Outcome {
        human,
        json: vec![
            ("action", Json::string(LodAction::Gen.name())),
            ("path", Json::string(output.display().to_string())),
            ("bytes", Json::Number(bytes.len() as i64)),
            ("node", Json::Number(node as i64)),
            ("mesh", Json::Number(mesh as i64)),
            ("primitive", Json::Number(primitive as i64)),
            ("levels", Json::Number(measured.levels.len() as i64)),
            ("dag", dag_json(&measured)),
        ],
    })
}

/// The one node of the scene that draws a mesh, or a refusal naming the rest.
fn only_node(scene: &GltfScene, args: &LodArgs) -> Result<usize, Failure> {
    let nodes = selection(scene, args);
    match nodes.as_slice() {
        [only] => Ok(*only),
        [] => Err(Failure::new(format!(
            "{} draws no mesh, so there is no DAG to generate",
            args.file.display()
        ))),
        several => Err(Failure::new(format!(
            "{} draws {} meshes and one .dag holds one of them; pass --node <INDEX> — {}",
            args.file.display(),
            several.len(),
            several
                .iter()
                .map(|&node| node_label(scene, node))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .with("nodes", Json::Number(several.len() as i64))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The threshold is a share, and both ends of it decide something.
    #[test]
    fn a_level_that_kept_more_than_the_threshold_is_stalled() {
        let level = |kept: Option<f32>| DagLevel {
            triangles: 0,
            clusters: 0,
            groups: 0,
            error: None,
            kept,
        };
        assert!(!level(None).stalled(), "level 0 is below nothing");
        assert!(!level(Some(0.5)).stalled(), "a clean halving");
        assert!(!level(Some(STALL_KEPT)).stalled(), "the bound is a cap");
        assert!(level(Some(STALL_KEPT + 0.01)).stalled());
        assert!(level(Some(1.0)).stalled(), "nothing was removed at all");
    }

    /// A range that spans nothing is `-`, and a value too small for four
    /// decimals is not printed as zero.
    #[test]
    fn an_error_range_says_what_it_has() {
        assert_eq!(error_text(None), "-");
        assert_eq!(error_text(Some((0.0, 0.0))), "0.0000..0.0000");
        // Exactly representable and exactly half way, so it rounds to even.
        assert_eq!(error_text(Some((0.0, 0.03125))), "0.0000..0.0312");
        assert_eq!(
            error_text(Some((1e-7, 2e-5))),
            "1.00e-7..2.00e-5",
            "a mesh modelled in metres still reports its error"
        );
    }

    #[test]
    fn the_smallest_and_largest_of_nothing_is_no_range() {
        assert_eq!(range(std::iter::empty()), None);
        assert_eq!(range([2.0f32, 0.5, 1.0].into_iter()), Some((0.5, 2.0)));
    }
}
