//! The glTF documents the tests import, built here rather than checked in.
//!
//! A `.glb` is a binary container and a `.gltf` full of base64 is not much
//! better, so a vendored sample would be a blob nobody reviewing a change could
//! read — the same objection `crcbl-sprite` records against binary `.crpix`
//! fixtures. Everything under test is instead assembled from JSON text and a
//! byte layout spelled out below, so a diff that changes what is tested shows
//! what changed.
//!
//! [`triangle_json`] is a single triangle: three vertices carrying positions,
//! normals and `TEXCOORD_0`, three `u16` indices, one material, and two nodes
//! so that transform composition has a parent to compose with. Malformed cases
//! are that document with one thing altered, through `replacing`, which
//! refuses to alter nothing.
//!
//! `lod_glb` is the second shape, for [`crate::lod_resolve`]: a node and a
//! mesh per entry, so a chain of levels is a list of meshes and the names,
//! `MSFT_lod` ids and scene membership that tie them together. It builds its
//! buffer from the arrays it is given rather than from constants, because what
//! its tests assert is that a level's geometry came out the same as it went
//! in.
//!
//! # The `gltf-fixture` feature, and what it does *not* make public
//!
//! The same argument reaches outside this crate. A **gate** that has to point a
//! tool at a `.glb` — `tools/run-samples-windowed.sh` pointing `viewer` at one
//! — has the same two options and the same answer: a committed blob is
//! unreadable, so the document is generated, and generating it means this
//! builder rather than a third transcription of the glTF spec beside the two
//! already here.
//!
//! So the `gltf-fixture` feature compiles this module outside `cfg(test)` and
//! makes exactly four items public: [`triangle_json`] and its two halves,
//! [`BIN_CHUNK_BUFFER`] and [`triangle_bin`], plus [`glb`], the container that
//! closes them into a file. That is a whole `.glb` and nothing else. Everything
//! the tests reach for — the malformed-case machinery, the LOD and textured
//! shapes, the `tempfile`-backed asset directory — stays `#[cfg(test)]` and
//! `pub(crate)`, because a caller outside this crate has no use for a document
//! built to be refused, and because those halves are built on this crate's
//! **dev**-dependencies, which a feature cannot turn on.
//!
//! **It is not part of the engine's API.** Nothing in `crcbl-scene`'s own
//! surface takes or returns these bytes, no default build compiles them, and
//! the feature exists so that gates and tests in other crates can ask for a
//! document this crate already knows how to write. Treat a change to it as a
//! change to a test helper.

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crcbl_assets::{DirSource, StorageError};

#[cfg(test)]
use crate::{GltfScene, import_gltf};

/// The buffer clause of a `.glb`: the bytes are the container's `BIN` chunk, so
/// the buffer has no `uri`.
pub const BIN_CHUNK_BUFFER: &str = r#"{ "byteLength": 102 }"#;

/// The buffer clause of a `.gltf`: the bytes are a file beside the document.
#[cfg(test)]
pub(crate) const EXTERNAL_BUFFER: &str = r#"{ "byteLength": 102, "uri": "triangle.bin" }"#;

/// The bytes every accessor in [`triangle_json`] reads, in the order the buffer
/// views slice them: positions, normals, texture coordinates, indices.
///
/// | bytes    | what              |
/// | -------- | ----------------- |
/// | `0..36`  | 3 × `[f32; 3]` position |
/// | `36..72` | 3 × `[f32; 3]` normal |
/// | `72..96` | 3 × `[f32; 2]` texcoord |
/// | `96..102`| 3 × `u16` index |
#[must_use]
pub fn triangle_bin() -> Vec<u8> {
    let mut bytes = Vec::new();
    for position in POSITIONS {
        for component in position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for normal in NORMALS {
        for component in normal {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for uv in TEX_COORDS {
        for component in uv {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    for index in INDICES {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

pub(crate) const POSITIONS: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
pub(crate) const NORMALS: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
pub(crate) const TEX_COORDS: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
pub(crate) const INDICES: [u16; 3] = [0, 1, 2];
/// The factor `triangle_json`'s one material declares.
#[cfg(test)]
pub(crate) const BASE_COLOR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The document, with `buffer` as its single buffer's JSON object.
#[must_use]
pub fn triangle_json(buffer: &str) -> String {
    format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [
    {{ "name": "root", "translation": [10.0, 0.0, 0.0], "children": [1] }},
    {{ "name": "leaf", "mesh": 0, "translation": [0.0, 5.0, 0.0] }}
  ],
  "meshes": [{{
    "name": "triangle",
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }},
      "indices": 3,
      "material": 0
    }}]
  }}],
  "materials": [{{
    "name": "paint",
    "pbrMetallicRoughness": {{ "baseColorFactor": [0.25, 0.5, 0.75, 1.0] }}
  }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 72, "byteLength": 24 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 6 }}
  ],
  "buffers": [{buffer}]
}}"#
    )
}

/// `json` with the one occurrence of `from` replaced by `to`.
///
/// Asserts that `from` occurs exactly once, because a mutation that silently
/// matched nothing would leave the fixture valid and the "this is refused" test
/// asserting that a well-formed document is refused — which it would then fail
/// to do, quietly, forever.
#[cfg(test)]
pub(crate) fn replacing(json: &str, from: &str, to: &str) -> String {
    assert_eq!(
        json.matches(from).count(),
        1,
        "{from:?} must appear exactly once in the fixture to be replaced"
    );
    json.replace(from, to)
}

/// A `.glb` container around `json`, padded exactly as the format requires:
/// the `JSON` chunk to a multiple of four with spaces, the `BIN` chunk with
/// zeroes.
#[must_use]
pub fn glb(json: &str, bin: Option<&[u8]>) -> Vec<u8> {
    let mut json_chunk = json.as_bytes().to_vec();
    while !json_chunk.len().is_multiple_of(4) {
        json_chunk.push(b' ');
    }
    let bin_chunk = bin.map(|bin| {
        let mut chunk = bin.to_vec();
        while !chunk.len().is_multiple_of(4) {
            chunk.push(0);
        }
        chunk
    });

    let total = 12 + 8 + json_chunk.len() + bin_chunk.as_ref().map_or(0, |chunk| 8 + chunk.len());
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(total).unwrap().to_le_bytes());
    out.extend_from_slice(&u32::try_from(json_chunk.len()).unwrap().to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_chunk);
    if let Some(chunk) = bin_chunk {
        out.extend_from_slice(&u32::try_from(chunk.len()).unwrap().to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&chunk);
    }
    out
}

/// One node of a [`lod_glb`] document, and the mesh it draws.
///
/// Enough of a glTF to carry a LOD chain and nothing else: a name for the
/// `name_LOD1` convention, a triangle list so a level's geometry is
/// recognisable, and `MSFT_lod` ids for the extension half.
#[cfg(test)]
pub(crate) struct LodNode<'a> {
    /// The node's `name`.
    pub(crate) name: &'a str,
    /// Positions and indices of the mesh it draws, or `None` for a node that
    /// draws nothing.
    pub(crate) geometry: Option<(&'a [[f32; 3]], &'a [u32])>,
    /// The node's `MSFT_lod` `ids`, written verbatim into the JSON so that a
    /// malformed one can be a fixture too.
    pub(crate) lod_ids: Option<&'a str>,
}

#[cfg(test)]
impl<'a> LodNode<'a> {
    /// A node drawing `mesh`, with no `MSFT_lod`.
    pub(crate) fn new(name: &'a str, mesh: &'a (Vec<[f32; 3]>, Vec<u32>)) -> Self {
        Self {
            name,
            geometry: Some((&mesh.0, &mesh.1)),
            lod_ids: None,
        }
    }

    /// The same node with an `MSFT_lod` extension whose body is `ids`.
    pub(crate) fn msft_lod(mut self, ids: &'a str) -> Self {
        self.lod_ids = Some(ids);
        self
    }

    /// A node with a name and no mesh.
    pub(crate) fn empty(name: &'a str) -> Self {
        Self {
            name,
            geometry: None,
            lod_ids: None,
        }
    }
}

/// A `.glb` of one node per entry of `nodes`, each drawing its own mesh, with
/// `scene` naming the ones the default scene contains.
///
/// `scene` is separate because `MSFT_lod`'s lower levels are deliberately kept
/// out of every scene — a loader that drew them would draw the mesh several
/// times over — so a fixture that put every node in the scene could not
/// exercise the case the extension is actually written for.
#[cfg(test)]
pub(crate) fn lod_glb(nodes: &[LodNode<'_>], scene: &[usize]) -> Vec<u8> {
    let mut bin = Vec::new();
    let mut meshes = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let mut node_json = Vec::new();

    for node in nodes {
        let mesh = node.geometry.map(|(positions, indices)| {
            let mesh = meshes.len();
            let position_view = views.len();
            views.push(format!(
                r#"{{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}"#,
                bin.len(),
                positions.len() * 12
            ));
            for position in positions {
                for component in position {
                    bin.extend_from_slice(&component.to_le_bytes());
                }
            }
            views.push(format!(
                r#"{{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}"#,
                bin.len(),
                indices.len() * 4
            ));
            for index in indices {
                bin.extend_from_slice(&index.to_le_bytes());
            }
            accessors.push(format!(
                r#"{{ "bufferView": {position_view}, "componentType": 5126, "count": {}, "type": "VEC3" }}"#,
                positions.len()
            ));
            accessors.push(format!(
                r#"{{ "bufferView": {}, "componentType": 5125, "count": {}, "type": "SCALAR" }}"#,
                position_view + 1,
                indices.len()
            ));
            meshes.push(format!(
                r#"{{ "primitives": [{{ "attributes": {{ "POSITION": {} }}, "indices": {} }}] }}"#,
                position_view,
                position_view + 1
            ));
            mesh
        });

        let mut fields = vec![format!(r#""name": "{}""#, node.name)];
        if let Some(mesh) = mesh {
            fields.push(format!(r#""mesh": {mesh}"#));
        }
        if let Some(ids) = node.lod_ids {
            fields.push(format!(r#""extensions": {{ "MSFT_lod": {ids} }}"#));
        }
        node_json.push(format!("{{ {} }}", fields.join(", ")));
    }

    let scene: Vec<String> = scene.iter().map(usize::to_string).collect();
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [{}] }}],
  "nodes": [{}],
  "meshes": [{}],
  "accessors": [{}],
  "bufferViews": [{}],
  "buffers": [{{ "byteLength": {} }}]
}}"#,
        scene.join(", "),
        node_json.join(", "),
        meshes.join(", "),
        accessors.join(", "),
        views.join(", "),
        bin.len(),
    );
    glb(&json, Some(&bin))
}

/// The four texels [`textured_glb`]'s image carries, RGBA8 row-major.
///
/// No two alike and none of them grey, so a layer that arrived flipped, halved,
/// or from the wrong image is a different set of numbers rather than the same
/// one — the argument `crcbl_render::scene::CHECKER_TEXELS` records for the
/// engine's own page.
#[cfg(test)]
pub(crate) const IMAGE_TEXELS: [u8; 16] = [
    0xFF, 0x00, 0x00, 0xFF, // (0, 0) red
    0x00, 0xFF, 0x00, 0xFF, // (1, 0) green
    0x00, 0x00, 0xFF, 0xFF, // (0, 1) blue
    0xFF, 0xFF, 0x00, 0xFF, // (1, 1) yellow
];

/// `pixels` as a `width`×`height` RGBA8 PNG.
///
/// Encoded rather than checked in, for this module's opening argument: a
/// vendored binary is a fixture nobody reviewing a change can read, where four
/// named texels above are.
#[cfg(test)]
pub(crate) fn png_bytes(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
    }
    bytes
}

/// [`textured_parts`] closed into a `.glb`, which is what a test that does not
/// mean to mutate the document wants.
#[cfg(test)]
pub(crate) fn textured_glb(image: &[u8], mime: &str, tex_coord: u32) -> Vec<u8> {
    let (json, bin) = textured_parts(image, mime, tex_coord);
    glb(&json, Some(&bin))
}

/// The JSON and the `BIN` chunk of a one-triangle document whose material
/// carries a `baseColorTexture`, with the image's bytes in a `bufferView`.
///
/// `image` is the encoded bytes — [`png_bytes`] for the usual case, and anything
/// at all for the "this is not a format we decode" ones. `mime` is what the
/// document *claims* they are, which is deliberately separable from what they
/// are. `tex_coord` is the material's `texCoord`, so a set this importer does
/// not read is a fixture too.
///
/// The two halves are returned separately rather than as a container so a
/// refusal test can put one thing wrong in the JSON with [`replacing`] and close
/// it with [`glb`] afterwards — the same shape [`triangle_json`] has, and for
/// the same reason.
#[cfg(test)]
pub(crate) fn textured_parts(image: &[u8], mime: &str, tex_coord: u32) -> (String, Vec<u8>) {
    let mut bin = triangle_bin();
    // The spec requires a bufferView holding image data to be four-byte aligned
    // like any other, and an unaligned one would be this fixture's bug rather
    // than the importer's.
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let image_offset = bin.len();
    bin.extend_from_slice(image);

    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0] }}],
  "nodes": [
    {{ "name": "root", "translation": [10.0, 0.0, 0.0], "children": [1] }},
    {{ "name": "leaf", "mesh": 0, "translation": [0.0, 5.0, 0.0] }}
  ],
  "meshes": [{{
    "name": "triangle",
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }},
      "indices": 3,
      "material": 0
    }}]
  }}],
  "materials": [{{
    "name": "painted",
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.25, 0.5, 0.75, 1.0],
      "baseColorTexture": {{ "index": 0, "texCoord": {tex_coord} }}
    }}
  }}],
  "textures": [{{ "source": 0 }}],
  "images": [{{ "name": "paint", "bufferView": 4, "mimeType": "{mime}" }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 72, "byteLength": 24 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 6 }},
    {{ "buffer": 0, "byteOffset": {image_offset}, "byteLength": {} }}
  ],
  "buffers": [{{ "byteLength": {} }}]
}}"#,
        image.len(),
        bin.len(),
    );
    (json, bin)
}

/// A directory of assets, read through the real [`DirSource`] rather than a
/// mock — so the key rule a buffer URI has to satisfy is the one that will
/// apply in production, not one written for the test.
#[cfg(test)]
pub(crate) struct Assets {
    dir: tempfile::TempDir,
}

#[cfg(test)]
impl Assets {
    pub(crate) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("assets/meshes")).unwrap();
        Self { dir }
    }

    /// The directory the source is *not* rooted at, one level above it. A test
    /// that wants a file an escaping URI would reach puts it here.
    pub(crate) fn outside(&self) -> &Path {
        self.dir.path()
    }

    pub(crate) fn write(&self, key: &str, bytes: &[u8]) {
        std::fs::write(self.dir.path().join("assets").join(key), bytes).unwrap();
    }

    pub(crate) fn import(&self, key: &str) -> Result<GltfScene, StorageError> {
        let source = DirSource::at(self.dir.path().join("assets"));
        import_gltf(&source, Path::new(key))
    }
}

/// Import `json` as a `.glb` whose `BIN` chunk is [`triangle_bin`].
#[cfg(test)]
pub(crate) fn import_glb(json: &str) -> Result<GltfScene, StorageError> {
    import_glb_bytes(&glb(json, Some(&triangle_bin())))
}

/// [`import_glb`] for the rigged document, whose buffer is a different length.
#[cfg(test)]
pub(crate) fn import_rigged_glb(json: &str) -> Result<GltfScene, StorageError> {
    import_glb_bytes(&glb(json, Some(&rigged_bin())))
}

/// Import raw bytes as `meshes/model.glb`.
#[cfg(test)]
pub(crate) fn import_glb_bytes(bytes: &[u8]) -> Result<GltfScene, StorageError> {
    let assets = Assets::new();
    assets.write("meshes/model.glb", bytes);
    assets.import("meshes/model.glb")
}

/// Import `json` as `meshes/model.gltf`, with `bin` written beside it as the
/// `triangle.bin` its buffer names.
#[cfg(test)]
pub(crate) fn import_gltf_text(json: &str, bin: &[u8]) -> Result<GltfScene, StorageError> {
    let assets = Assets::new();
    assets.write("meshes/model.gltf", json.as_bytes());
    assets.write("meshes/triangle.bin", bin);
    assets.import("meshes/model.gltf")
}

// ---------------------------------------------------------------------------
// The rigged fixture
// ---------------------------------------------------------------------------

/// Which joint each of the triangle's three vertices is bound to.
///
/// `JOINTS_0` is `VEC4` of `UNSIGNED_BYTE` or `UNSIGNED_SHORT` per the
/// specification — never float — and the three slots past the first carry zero
/// weight, so only the leading index matters.
pub(crate) const JOINTS: [[u16; 4]; 3] = [[0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]];

/// How much of each vertex each joint owns. Rigid: one joint takes all of it.
pub(crate) const WEIGHTS: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0]; 3];

/// The two joints' inverse bind matrices, column-major as the format stores
/// them.
///
/// The root binds at the origin, so its inverse is the identity. The tip binds
/// one unit up — `"translation": [0, 1, 0]` on its node — so its inverse
/// translates one unit down, and the last column is where a column-major matrix
/// keeps that.
pub(crate) const INVERSE_BIND: [[f32; 16]; 2] = [
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ],
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, -1.0, 0.0, 1.0,
    ],
];

/// The clip's two keyframe times, in seconds.
pub(crate) const CLIP_TIMES: [f32; 2] = [0.0, 1.0];

/// What the tip joint's rotation is at each of those times, as `xyzw`
/// quaternions — the order the format stores them in.
///
/// Identity, then a quarter turn about Z. `sin` and `cos` of forty-five degrees
/// rather than a decimal typed from memory: a quaternion for an angle `θ` carries
/// `θ/2` in it, which is the step this fixture would otherwise get wrong
/// silently.
pub(crate) const CLIP_ROTATIONS: [[f32; 4]; 2] = [
    [0.0, 0.0, 0.0, 1.0],
    [
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ],
];

/// The bytes [`rigged_json`]'s accessors read.
///
/// | bytes       | what                                    |
/// | ----------- | --------------------------------------- |
/// | `0..102`    | the same geometry [`triangle_bin`] holds |
/// | `102..104`  | two bytes of padding, so what follows is four-byte aligned |
/// | `104..128`  | 3 × `[u16; 4]` joint indices             |
/// | `128..176`  | 3 × `[f32; 4]` joint weights            |
/// | `176..304`  | 2 × `[f32; 16]` inverse bind matrices    |
/// | `304..312`  | 2 × `f32` keyframe times                |
/// | `312..344`  | 2 × `[f32; 4]` keyframe rotations        |
#[must_use]
pub fn rigged_bin() -> Vec<u8> {
    let mut bytes = triangle_bin();
    // The accessors below are all four-byte types, and the specification puts
    // each one's offset at a multiple of its component size. The indices left
    // the cursor on an even address rather than a multiple of four.
    bytes.resize(104, 0);
    for joints in JOINTS {
        for index in joints {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
    }
    for weights in WEIGHTS {
        for weight in weights {
            bytes.extend_from_slice(&weight.to_le_bytes());
        }
    }
    for matrix in INVERSE_BIND {
        for cell in matrix {
            bytes.extend_from_slice(&cell.to_le_bytes());
        }
    }
    for time in CLIP_TIMES {
        bytes.extend_from_slice(&time.to_le_bytes());
    }
    for rotation in CLIP_ROTATIONS {
        for component in rotation {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// A document with a skin and one animation, with `buffer` as its single
/// buffer's JSON object.
///
/// Deliberately the smallest thing that is still a rig: one skinned primitive,
/// two joints in a parent/child pair, and a clip that turns the child. Every
/// piece the importer has to read is present exactly once, so a test that loses
/// one of them fails on that one rather than on a document that stopped being
/// valid glTF.
///
/// Two accessors here carry `min` and `max` because the specification requires
/// them rather than because anything reads them: an animation sampler's `input`,
/// so a player knows a clip's duration without walking its samples, and
/// `POSITION`, so a loader can bound a mesh without reading its vertices. This
/// document validates under `gltf::Gltf::from_slice`, which is stricter than the
/// `from_slice_without_validation` the importer itself uses — the fixture is
/// held to the format, not merely to what this crate happens to accept.
#[must_use]
pub fn rigged_json(buffer: &str) -> String {
    format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0, 1] }}],
  "nodes": [
    {{ "name": "skinned", "mesh": 0, "skin": 0 }},
    {{ "name": "joint-root", "children": [2] }},
    {{ "name": "joint-tip", "translation": [0.0, 1.0, 0.0] }}
  ],
  "meshes": [{{
    "name": "rigged-triangle",
    "primitives": [{{
      "attributes": {{
        "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2,
        "JOINTS_0": 4, "WEIGHTS_0": 5
      }},
      "indices": 3,
      "material": 0
    }}]
  }}],
  "skins": [{{
    "name": "rig",
    "skeleton": 1,
    "joints": [1, 2],
    "inverseBindMatrices": 6
  }}],
  "animations": [{{
    "name": "wave",
    "channels": [{{ "sampler": 0, "target": {{ "node": 2, "path": "rotation" }} }}],
    "samplers": [{{ "input": 7, "output": 8, "interpolation": "LINEAR" }}]
  }}],
  "materials": [{{
    "name": "paint",
    "pbrMetallicRoughness": {{ "baseColorFactor": [0.25, 0.5, 0.75, 1.0] }}
  }}],
  "accessors": [
    {{
      "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
      "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
    }},
    {{ "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }},
    {{ "bufferView": 4, "componentType": 5123, "count": 3, "type": "VEC4" }},
    {{ "bufferView": 5, "componentType": 5126, "count": 3, "type": "VEC4" }},
    {{ "bufferView": 6, "componentType": 5126, "count": 2, "type": "MAT4" }},
    {{
      "bufferView": 7, "componentType": 5126, "count": 2, "type": "SCALAR",
      "min": [0.0], "max": [1.0]
    }},
    {{ "bufferView": 8, "componentType": 5126, "count": 2, "type": "VEC4" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 72, "byteLength": 24 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 6 }},
    {{ "buffer": 0, "byteOffset": 104, "byteLength": 24 }},
    {{ "buffer": 0, "byteOffset": 128, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 176, "byteLength": 128 }},
    {{ "buffer": 0, "byteOffset": 304, "byteLength": 8 }},
    {{ "buffer": 0, "byteOffset": 312, "byteLength": 32 }}
  ],
  "buffers": [{buffer}]
}}"#
    )
}
