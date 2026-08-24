//! The `.glb` documents this crate's tests open, assembled here rather than
//! checked in.
//!
//! `crcbl-scene`'s own `gltf_fixture` makes the same argument and is
//! `pub(crate)` there, so it cannot be reached from an application;
//! `crates/crcbl/tests/gltf_e2e.rs` writes its own for the same reason. The
//! argument: a `.glb` is a binary container, so a vendored one is a fixture
//! nobody reviewing a change can read, and every number a test asserts on ought
//! to be a number written out in the file that asserts it.
//!
//! Everything here is one document — a one-metre quad under a translating node —
//! with one thing altered per case, so a test that fails says which alteration
//! did it.

use crcbl::math::Vec3;

/// Where [`quad_glb`]'s node puts the quad by default.
///
/// Off the origin on every axis, so a bounds computation that dropped the node
/// transform lands somewhere a test can see — an origin-centred fixture agrees
/// with a broken composition and a correct one.
pub const QUAD_CENTRE: Vec3 = Vec3::new(2.0, 1.0, -3.0);

/// The quad's corners in its own space: one metre across, in the `XY` plane,
/// facing `+Z`.
const POSITIONS: [[f32; 3]; 4] = [
    [-0.5, -0.5, 0.0],
    [0.5, -0.5, 0.0],
    [0.5, 0.5, 0.0],
    [-0.5, 0.5, 0.0],
];

/// One normal per corner, all facing `+Z`.
const NORMALS: [[f32; 3]; 4] = [[0.0, 0.0, 1.0]; 4];

/// Two triangles, counter-clockwise seen from `+Z`, which is the front face.
const INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// A `.glb` of one quad, on a node that translates it to `centre`.
#[must_use]
pub fn quad_glb(centre: Vec3) -> Vec<u8> {
    document(&node_clause(centre, true, Vec3::ONE), POSITIONS)
}

/// A `.glb` whose scene's only node draws nothing.
///
/// Legal glTF and the shape a file of loose meshes with no arrangement takes:
/// the mesh is in the document and no node places it, so there is nothing to
/// frame and nothing to draw.
#[must_use]
pub fn empty_glb() -> Vec<u8> {
    document(&node_clause(Vec3::ZERO, false, Vec3::ONE), POSITIONS)
}

/// A `.glb` whose one primitive is drawn as `POINTS`.
///
/// **A document that loads and converts to nothing.** The positions are there,
/// so the bounding box is finite and [`crate::model::load`] is happy with it;
/// `build_render_scene` draws no point lists, so it skips the primitive and
/// hands back no instances at all. That gap between "has geometry" and "has
/// something to draw" is the one `crate::app`'s reload refuses on, and nothing
/// else in this module reaches it — [`empty_glb`] is refused a step earlier,
/// at the bounds.
#[must_use]
pub fn points_glb() -> Vec<u8> {
    document_with(
        std::slice::from_ref(&node_clause(Vec3::ZERO, true, Vec3::ONE)),
        POSITIONS,
        POINTS_MODE,
        false,
    )
}

/// glTF's `mode` for a point list — table 24 of the specification. The default
/// is `4`, triangles, which is what every other document here leaves out.
const POINTS_MODE: &str = r#""mode": 0, "#;

/// A `.glb` whose one node scales its axes unequally.
///
/// **A document that loads and draws, and still has something to report.** The
/// conversion places the instance — the geometry is in the right place and
/// lights correctly — and reports a `scale` skip, because a scaled cone is no
/// longer a cone, so the mesh path skips the per-cluster back-face test for
/// this instance and culls it by its bounding sphere alone. Nothing is
/// missing; it costs more to draw than its geometry says it should. That is
/// the case a listing panel is really for: a file that arrived intact and
/// still has a caveat attached.
#[must_use]
pub fn skewed_glb() -> Vec<u8> {
    document(
        &node_clause(Vec3::ZERO, true, Vec3::new(1.0, 2.0, 1.0)),
        POSITIONS,
    )
}

/// A `.glb` with one corner at infinity.
///
/// Nothing in the importer looks at a float's *value*, so this is a document
/// that loads cleanly and has no bounding box — see [`crate::model`].
///
/// Infinity survives the fold on ordering, so the corner may sit anywhere —
/// unlike [`nan_glb`], where the position is the whole point.
#[must_use]
pub fn non_finite_glb() -> Vec<u8> {
    let mut positions = POSITIONS;
    positions[0][1] = f32::INFINITY;
    document(&node_clause(Vec3::ZERO, true, Vec3::ONE), positions)
}

/// A `.glb` with one `NaN` corner.
///
/// The index does not matter, and that is the point: `Aabb::from_points` skips
/// a `NaN` lane from every position, so this document's bounding box comes out
/// finite and indistinguishable from a healthy one. It is refused by
/// `crate::model`'s position scan instead — which is what makes this fixture
/// the thing that proves the scan runs.
#[must_use]
pub fn nan_glb() -> Vec<u8> {
    let mut positions = POSITIONS;
    positions[2][0] = f32::NAN;
    document(&node_clause(Vec3::ZERO, true, Vec3::ONE), positions)
}

/// A `.glb` whose scene places the same mesh twice.
///
/// **The one fixture here with an instance count other than one.** A reload
/// that swapped the renderer's scene and left every count behind would agree
/// with a one-instance document at both ends; this is what makes the numbers on
/// the listing panel and the debug row observable at all.
#[must_use]
pub fn two_quads_glb() -> Vec<u8> {
    document_with(
        &[
            node_clause(Vec3::ZERO, true, Vec3::ONE),
            node_clause(Vec3::new(3.0, 0.0, 0.0), true, Vec3::ONE),
        ],
        POSITIONS,
        "",
        false,
    )
}

/// A `.glb` carrying one animation, and the animation carries no name.
///
/// glTF does not require an `animations` entry to be named, so a document out
/// of an exporter that did not write one is ordinary rather than broken — and
/// [`crate::model::Rig`] has to have something to call it. This is the only
/// fixture that reaches that stand-in, so without it the name `crate::model`
/// substitutes is text no test ever reads.
///
/// **No skin, deliberately.** A document can animate a node without rigging
/// anything, so this is a rig whose joint count is zero and whose clip list is
/// not — which is the shape [`crate::model::Rig::is_empty`] has to get right,
/// and the one a fixture with both would hide.
#[must_use]
pub fn unnamed_clip_glb() -> Vec<u8> {
    document_with(
        std::slice::from_ref(&node_clause(Vec3::ZERO, true, Vec3::ONE)),
        POSITIONS,
        "",
        true,
    )
}

/// [`unnamed_clip_glb`]'s keyframe times, in seconds.
const CLIP_TIMES: [f32; 2] = [0.0, 1.0];

/// Its rotation at each of those times, as `xyzw` quaternions: identity, then
/// a half turn about `Z`.
///
/// Nothing asserts on these values. A sampler needs keyframes and the
/// specification requires at least one, so they are the smallest pair that is
/// still a curve.
const CLIP_ROTATIONS: [[f32; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0]];

/// The two joint nodes [`skinned_glb`] declares, as indices into its `nodes`
/// array: the root at the origin, and its child a metre above it.
///
/// Written down because three places have to agree about them — the skin's
/// `joints`, the root's `children`, and the animation channel's target.
const SKIN_JOINT_NODES: [usize; 2] = [1, 2];

/// How far the tip joint stands above its parent, in metres.
const SKIN_TIP_Y: f32 = 1.0;

/// A `.glb` with a skin over two joints and a clip that drives one of them and
/// the mesh node too.
///
/// **The one fixture here that is a rig**, and it exists for the two cases
/// [`crate::anim`] has to get right and that no other document in this crate
/// shows:
///
/// * Its clip has **two** channels — one on the tip joint and one on the mesh
///   node, which is not a joint of the skin at all. That is what a clip out of
///   any tool looks like, since one animation drives the whole scene, and the
///   second channel is the one a converter has to drop rather than hand to a
///   joint index that means something else.
/// * `parents_first` spells the joint array both ways. `true` is the ordinary
///   document, root then tip. `false` lists the tip first, which is a hierarchy
///   `Skeleton::new` refuses because its single forward pass would read a
///   parent it has not reached — and the viewer's answer to that is a message
///   rather than a renumbering, for the reason [`crate::anim`] gives.
///
/// The inverse bind matrices follow the joint order in both spellings, so the
/// document is a correct rig either way and the ordering is the only thing that
/// changes between them.
#[must_use]
pub fn skinned_glb(parents_first: bool) -> Vec<u8> {
    let [root, tip] = SKIN_JOINT_NODES;
    let (joints, binds) = if parents_first {
        (
            [root, tip],
            [inverse_bind_at(0.0), inverse_bind_at(SKIN_TIP_Y)],
        )
    } else {
        (
            [tip, root],
            [inverse_bind_at(SKIN_TIP_Y), inverse_bind_at(0.0)],
        )
    };

    // Every offset below is where the previous array ended, and each is a
    // multiple of its own component size — four for a float, two for an index —
    // which is what glTF requires of a buffer view an accessor reads.
    let mut bin = Vec::new();
    for value in POSITIONS.iter().chain(NORMALS.iter()).flatten() {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let indices_at = bin.len();
    for index in INDICES {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    // Rigid bindings: the half of the quad above the origin on the tip joint,
    // the half below it on the root, and the whole of each vertex on its one
    // joint. `JOINTS_0` indexes the skin's own `joints` array, so joint 1 is
    // whichever node that array lists second.
    let joints_at = bin.len();
    for [_, y, _] in POSITIONS {
        for slot in [u16::from(y >= 0.0), 0, 0, 0] {
            bin.extend_from_slice(&slot.to_le_bytes());
        }
    }
    let weights_at = bin.len();
    for _ in POSITIONS {
        for weight in [1.0_f32, 0.0, 0.0, 0.0] {
            bin.extend_from_slice(&weight.to_le_bytes());
        }
    }
    let binds_at = bin.len();
    for value in binds.iter().flatten() {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let times_at = bin.len();
    for time in CLIP_TIMES {
        bin.extend_from_slice(&time.to_le_bytes());
    }
    let rotations_at = bin.len();
    for component in CLIP_ROTATIONS.iter().flatten() {
        bin.extend_from_slice(&component.to_le_bytes());
    }
    // The mesh node's own curve, so the clip drives something outside the skin.
    let translations_at = bin.len();
    for component in [[0.0_f32, 0.0, 0.0], [0.0, 0.0, 1.0]].iter().flatten() {
        bin.extend_from_slice(&component.to_le_bytes());
    }
    let end = bin.len();

    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0, {root}] }}],
  "nodes": [
    {{ "name": "panel", "mesh": 0, "skin": 0 }},
    {{ "name": "root", "children": [{tip}] }},
    {{ "name": "tip", "translation": [0.0, {SKIN_TIP_Y}, 0.0] }}
  ],
  "meshes": [{{
    "name": "panel",
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1, "JOINTS_0": 3, "WEIGHTS_0": 4 }},
      "indices": 2,
      "material": 0
    }}]
  }}],
  "skins": [{{
    "name": "rig",
    "skeleton": {root},
    "joints": [{}, {}],
    "inverseBindMatrices": 5
  }}],
  "animations": [{{
    "name": "wave",
    "channels": [
      {{ "sampler": 0, "target": {{ "node": {tip}, "path": "rotation" }} }},
      {{ "sampler": 1, "target": {{ "node": 0, "path": "translation" }} }}
    ],
    "samplers": [
      {{ "input": 6, "output": 7, "interpolation": "LINEAR" }},
      {{ "input": 6, "output": 8, "interpolation": "LINEAR" }}
    ]
  }}],
  "materials": [{{
    "name": "paint",
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.8, 0.8, 0.8, 1.0],
      "metallicFactor": 0.0,
      "roughnessFactor": 1.0
    }}
  }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 4, "type": "VEC4" }},
    {{ "bufferView": 4, "componentType": 5126, "count": 4, "type": "VEC4" }},
    {{ "bufferView": 5, "componentType": 5126, "count": {}, "type": "MAT4" }},
    {{ "bufferView": 6, "componentType": 5126, "count": {}, "type": "SCALAR", "min": [{}], "max": [{}] }},
    {{ "bufferView": 7, "componentType": 5126, "count": {}, "type": "VEC4" }},
    {{ "bufferView": 8, "componentType": 5126, "count": 2, "type": "VEC3" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": {indices_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {joints_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {weights_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {binds_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {times_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {rotations_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {translations_at}, "byteLength": {} }}
  ],
  "buffers": [{{ "byteLength": {end} }}]
}}"#,
        joints[0],
        joints[1],
        binds.len(),
        CLIP_TIMES.len(),
        CLIP_TIMES[0],
        CLIP_TIMES[CLIP_TIMES.len() - 1],
        CLIP_ROTATIONS.len(),
        joints_at - indices_at,
        weights_at - joints_at,
        binds_at - weights_at,
        times_at - binds_at,
        rotations_at - times_at,
        translations_at - rotations_at,
        end - translations_at,
    );
    glb(&json, &bin)
}

/// The inverse bind matrix of a joint that binds `y` metres up the `Y` axis,
/// column-major, which is the order glTF stores a `MAT4` in.
///
/// The inverse of a translation is the opposite translation, and the last
/// column is where a column-major matrix keeps one — so this undoes exactly the
/// placement of the joint it belongs to, and the bind pose reproduces the mesh
/// as it is written.
fn inverse_bind_at(y: f32) -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, -y, 0.0, 1.0,
    ]
}

/// The document's one node: translated to `at`, scaled by `scale`, drawing the
/// mesh or not.
fn node_clause(at: Vec3, draws: bool, scale: Vec3) -> String {
    let mesh = if draws { r#""mesh": 0, "# } else { "" };
    format!(
        r#"{{ "name": "panel", {mesh}"translation": [{}, {}, {}], "scale": [{}, {}, {}] }}"#,
        at.x, at.y, at.z, scale.x, scale.y, scale.z
    )
}

/// The whole document, with `node` as its single node and `positions` as its
/// `POSITION` accessor's contents.
fn document(node: &str, positions: [[f32; 3]; 4]) -> Vec<u8> {
    document_with(
        std::slice::from_ref(&node.to_string()),
        positions,
        "",
        false,
    )
}

/// [`document`], with every node in `nodes` in one scene, `mode` spliced into
/// the mesh's one primitive, and an unnamed rotation clip on node 0 when
/// `animated`.
///
/// The clip's accessors and buffer views are appended only for a document that
/// declares one, so every other fixture here holds the accessors it always has
/// — this module's point is one alteration per case.
fn document_with(
    nodes: &[String],
    positions: [[f32; 3]; 4],
    mode: &str,
    animated: bool,
) -> Vec<u8> {
    let mut bin = Vec::new();
    for value in positions.iter().chain(NORMALS.iter()).flatten() {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let index_offset = bin.len();
    for index in INDICES {
        bin.extend_from_slice(&index.to_le_bytes());
    }

    let (mut animations, mut clip_accessors, mut clip_views) =
        (String::new(), String::new(), String::new());
    if animated {
        // The indices leave the cursor on a multiple of four, which is what the
        // float accessors below need their offsets to be.
        let times_at = bin.len();
        for time in CLIP_TIMES {
            bin.extend_from_slice(&time.to_le_bytes());
        }
        let rotations_at = bin.len();
        for component in CLIP_ROTATIONS.iter().flatten() {
            bin.extend_from_slice(&component.to_le_bytes());
        }
        // No `"name"` on the animation: that absence is the whole fixture.
        animations = r#"
  "animations": [{
    "channels": [{ "sampler": 0, "target": { "node": 0, "path": "rotation" } }],
    "samplers": [{ "input": 3, "output": 4, "interpolation": "LINEAR" }]
  }],"#
            .to_string();
        // `min` and `max` on a sampler's input because the specification
        // requires them there — see `apps/viewer/src/demo_model.rs`.
        clip_accessors = format!(
            r#",
    {{ "bufferView": 3, "componentType": 5126, "count": {}, "type": "SCALAR", "min": [{}], "max": [{}] }},
    {{ "bufferView": 4, "componentType": 5126, "count": {}, "type": "VEC4" }}"#,
            CLIP_TIMES.len(),
            CLIP_TIMES[0],
            CLIP_TIMES[CLIP_TIMES.len() - 1],
            CLIP_ROTATIONS.len(),
        );
        clip_views = format!(
            r#",
    {{ "buffer": 0, "byteOffset": {times_at}, "byteLength": {} }},
    {{ "buffer": 0, "byteOffset": {rotations_at}, "byteLength": {} }}"#,
            rotations_at - times_at,
            bin.len() - rotations_at,
        );
    }

    // `metallicFactor` is written out as zero, for the reason
    // `crates/crcbl/tests/gltf_e2e.rs` gives: glTF defaults a material to a
    // fully rough *conductor*, which has no diffuse lobe at all, so a fixture
    // that left it out would be nearly black in any frame drawn from it.
    let scene = (0..nodes.len())
        .map(|index| index.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let node = nodes.join(", ");
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [{scene}] }}],
  "nodes": [{node}],{animations}
  "meshes": [{{
    "name": "panel",
    "primitives": [{{
      {mode}"attributes": {{ "POSITION": 0, "NORMAL": 1 }},
      "indices": 2,
      "material": 0
    }}]
  }}],
  "materials": [{{
    "name": "paint",
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.8, 0.8, 0.8, 1.0],
      "metallicFactor": 0.0,
      "roughnessFactor": 1.0
    }}
  }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR" }}{clip_accessors}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": {index_offset}, "byteLength": 12 }}{clip_views}
  ],
  "buffers": [{{ "byteLength": {} }}]
}}"#,
        bin.len(),
    );
    glb(&json, &bin)
}

/// A `.glb` container around `json` and `bin`, padded as the format requires:
/// the `JSON` chunk to a multiple of four with spaces, the `BIN` chunk with
/// zeroes.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_chunk = json.as_bytes().to_vec();
    while !json_chunk.len().is_multiple_of(4) {
        json_chunk.push(b' ');
    }
    let mut bin_chunk = bin.to_vec();
    while !bin_chunk.len().is_multiple_of(4) {
        bin_chunk.push(0);
    }

    let total = 12 + 8 + json_chunk.len() + 8 + bin_chunk.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&length(total));
    out.extend_from_slice(&length(json_chunk.len()));
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_chunk);
    out.extend_from_slice(&length(bin_chunk.len()));
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin_chunk);
    out
}

/// A `.glb` length field: every one of them is a little-endian `u32`.
fn length(value: usize) -> [u8; 4] {
    u32::try_from(value)
        .expect("a fixture under 4 GiB")
        .to_le_bytes()
}
