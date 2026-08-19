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

/// A `.glb` whose one node scales its axes unequally.
///
/// **A document that loads, draws, and still lost something.** The conversion
/// places the instance — the geometry is in the right place — and reports a
/// `scale` skip, because the mesh shader transforms normals with the 3×3 part
/// of the transform and no inverse-transpose, so the object lights wrongly.
/// That is the case a listing panel is really for: a file that looks like it
/// arrived intact and did not.
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
    let mut bin = Vec::new();
    for value in positions.iter().chain(NORMALS.iter()).flatten() {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let index_offset = bin.len();
    for index in INDICES {
        bin.extend_from_slice(&index.to_le_bytes());
    }

    // `metallicFactor` is written out as zero, for the reason
    // `crates/crcbl/tests/gltf_e2e.rs` gives: glTF defaults a material to a
    // fully rough *conductor*, which has no diffuse lobe at all, so a fixture
    // that left it out would be nearly black in any frame drawn from it.
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [{node}],
  "meshes": [{{
    "name": "panel",
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1 }},
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
    {{ "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": {index_offset}, "byteLength": 12 }}
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
