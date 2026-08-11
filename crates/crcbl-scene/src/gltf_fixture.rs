//! The glTF documents the tests import, built here rather than checked in.
//!
//! A `.glb` is a binary container and a `.gltf` full of base64 is not much
//! better, so a vendored sample would be a blob nobody reviewing a change could
//! read — the same objection `crcbl-sprite` records against binary `.crpix`
//! fixtures. Everything under test is instead assembled from JSON text and a
//! byte layout spelled out below, so a diff that changes what is tested shows
//! what changed.
//!
//! The one document is a single triangle: three vertices carrying positions,
//! normals and `TEXCOORD_0`, three `u16` indices, one material, and two nodes
//! so that transform composition has a parent to compose with. Malformed cases
//! are that document with one thing altered, through [`replacing`], which
//! refuses to alter nothing.

use std::path::Path;

use crcbl_assets::{DirSource, StorageError};

use crate::{GltfScene, import_gltf};

/// The buffer clause of a `.glb`: the bytes are the container's `BIN` chunk, so
/// the buffer has no `uri`.
pub(crate) const BIN_CHUNK_BUFFER: &str = r#"{ "byteLength": 102 }"#;

/// The buffer clause of a `.gltf`: the bytes are a file beside the document.
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
pub(crate) fn triangle_bin() -> Vec<u8> {
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
pub(crate) const BASE_COLOR: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The document, with `buffer` as its single buffer's JSON object.
pub(crate) fn triangle_json(buffer: &str) -> String {
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
pub(crate) fn glb(json: &str, bin: Option<&[u8]>) -> Vec<u8> {
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

/// A directory of assets, read through the real [`DirSource`] rather than a
/// mock — so the key rule a buffer URI has to satisfy is the one that will
/// apply in production, not one written for the test.
pub(crate) struct Assets {
    dir: tempfile::TempDir,
}

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
pub(crate) fn import_glb(json: &str) -> Result<GltfScene, StorageError> {
    import_glb_bytes(&glb(json, Some(&triangle_bin())))
}

/// Import raw bytes as `meshes/model.glb`.
pub(crate) fn import_glb_bytes(bytes: &[u8]) -> Result<GltfScene, StorageError> {
    let assets = Assets::new();
    assets.write("meshes/model.glb", bytes);
    assets.import("meshes/model.glb")
}

/// Import `json` as `meshes/model.gltf`, with `bin` written beside it as the
/// `triangle.bin` its buffer names.
pub(crate) fn import_gltf_text(json: &str, bin: &[u8]) -> Result<GltfScene, StorageError> {
    let assets = Assets::new();
    assets.write("meshes/model.gltf", json.as_bytes());
    assets.write("meshes/triangle.bin", bin);
    assets.import("meshes/model.gltf")
}
