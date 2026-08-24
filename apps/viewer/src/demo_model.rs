//! The `.glb` the browser demo opens, generated here rather than shipped as a
//! file.
//!
//! # Why a demo needs a document at all
//!
//! Natively this application is pointed at a file: `viewer <MODEL>`, read
//! through a [`DirSource`](crcbl::assets::DirSource) rooted at that file's own
//! directory — see [`crate::model`]. In a browser there is no path to type and
//! no directory to root anything at, so a visitor who arrives at the demo with
//! nothing in hand would meet an empty viewport and a message about a file they
//! cannot supply. This module is what they see instead: a document that is
//! already in memory when the module starts, opened through a
//! [`MemorySource`](crcbl::assets::MemorySource) by the same
//! [`load_from`](crate::model::load_from) every other document goes through.
//!
//! # Why it is generated and not vendored
//!
//! `apps/viewer/src/fixture.rs` makes this argument for the test documents and
//! it holds here too: a `.glb` is a binary container, so a checked-in one is
//! bytes nobody reviewing a change can read, and every number that matters
//! about it — where the crate sits, how big the ground is, what colour either
//! is — would live somewhere no reader of this crate can see. Generated, the
//! document *is* the source you are reading, and it costs the wasm module a
//! function rather than a blob.
//!
//! It is also the one asset in this application that ships to a visitor, so it
//! is deliberately small: two primitives, two materials, no textures, no
//! external buffer. Nothing here is a demonstration of what the geometry
//! pipeline can do — `docs/plan/sample/05-viewer.md`'s point is that the viewer
//! opens *other people's* files, and the demo's job is only to be a correct one
//! that is already here.
//!
//! # It is a whole document, not a placeholder
//!
//! The listing panel behind `I` reports what the document holds — see
//! [`crate::listing`] — so a single grey box would make every row of it read
//! `1` and prove nothing about the panel. Hence two meshes at three nodes with
//! two visibly different materials: the mesh, node, material and instance
//! counts are all distinct from each other and all greater than one, and the
//! arrangement has depth for the orbit camera to turn around.

use std::array;

/// A face of an axis-aligned box: its outward normal, and the two in-plane axes
/// whose sweep orders the corners counter-clockwise seen from outside.
///
/// `u`, `v` and `normal` are a right-handed triple in that order — `u × v ==
/// normal` — which is what makes the one corner order below produce
/// front-facing triangles on all six faces rather than three of them.
struct Face {
    /// The outward normal, and the axis the face is offset along.
    normal: [f32; 3],
    /// The first in-plane axis.
    u: [f32; 3],
    /// The second in-plane axis.
    v: [f32; 3],
}

/// The `+Y` face, named because it is also the whole of the ground plane: a
/// flat quad facing up is one face of a box with no thickness, so
/// [`corners`] builds both from this.
const UP: Face = Face {
    normal: [0.0, 1.0, 0.0],
    u: [1.0, 0.0, 0.0],
    v: [0.0, 0.0, -1.0],
};

/// Every face of the crate, in no significant order.
const FACES: [Face; 6] = [
    Face {
        normal: [1.0, 0.0, 0.0],
        u: [0.0, 0.0, -1.0],
        v: [0.0, 1.0, 0.0],
    },
    Face {
        normal: [-1.0, 0.0, 0.0],
        u: [0.0, 0.0, 1.0],
        v: [0.0, 1.0, 0.0],
    },
    UP,
    Face {
        normal: [0.0, -1.0, 0.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 0.0, 1.0],
    },
    Face {
        normal: [0.0, 0.0, 1.0],
        u: [1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
    },
    Face {
        normal: [0.0, 0.0, -1.0],
        u: [-1.0, 0.0, 0.0],
        v: [0.0, 1.0, 0.0],
    },
];

/// Half the crate's size on each axis: a one-metre cube, which is the size
/// glTF's unit is metres makes it.
const CRATE_HALF: [f32; 3] = [0.5, 0.5, 0.5];

/// Half the ground plane's size. Zero on `Y` because it is a quad, not a slab —
/// the plane has no thickness and gets none from a scale, so the world box's
/// height comes from the crates alone.
const GROUND_HALF: [f32; 3] = [2.5, 0.0, 2.5];

/// How far the near crate is turned about `Y`, in degrees.
///
/// Off-axis on purpose: a box whose faces are parallel to the view planes is
/// the one arrangement where an orbit that does nothing and an orbit that works
/// look alike for the first few degrees.
const CRATE_YAW_DEGREES: f32 = 27.0;

/// The bytes of the demo document: a self-contained binary glTF.
///
/// One JSON chunk and one BIN chunk, with no `uri` anywhere in it — no external
/// buffer and no image — because a browser has no directory to resolve one
/// against. Every value in it is finite.
///
/// What is in it: a one-metre crate at two nodes, one of them turned and the
/// other smaller and set back, standing on a ground plane at a third. Two
/// materials, a warm one on the crates and a cool one on the ground, so the
/// listing panel's material rows differ by more than their index and the single
/// directional light has two albedos to separate.
///
/// Open it the way `apps/viewer/src/model.rs` documents: file it in a
/// [`MemorySource`](crcbl::assets::MemorySource) under the name it should be
/// reported by, and hand that to
/// [`load_from`](crate::model::load_from).
#[must_use]
pub fn demo_glb() -> Vec<u8> {
    let parts = [crate_part(), ground_part()];

    // The BIN chunk and the two JSON arrays that index into it are built in one
    // pass, because they have to agree: view `3n` holds part `n`'s positions,
    // `3n + 1` its normals, `3n + 2` its indices, and every accessor sits on the
    // view of the same number.
    let mut bin = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let mut meshes = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let (min, max) = position_bounds(&part.positions);
        let positions = push_vec3(&mut bin, &part.positions);
        let normals = push_vec3(&mut bin, &part.normals);
        let indices = push_u16(&mut bin, &part.indices);

        accessors.push(format!(
            r#"{{ "bufferView": {}, "componentType": 5126, "count": {}, "type": "VEC3", "min": {}, "max": {} }}"#,
            views.len(),
            part.positions.len(),
            vec3_json(min),
            vec3_json(max),
        ));
        views.push(view_json(&positions));
        accessors.push(format!(
            r#"{{ "bufferView": {}, "componentType": 5126, "count": {}, "type": "VEC3" }}"#,
            views.len(),
            part.normals.len(),
        ));
        views.push(view_json(&normals));
        accessors.push(format!(
            r#"{{ "bufferView": {}, "componentType": 5123, "count": {}, "type": "SCALAR" }}"#,
            views.len(),
            part.indices.len(),
        ));
        views.push(view_json(&indices));

        let first = index * 3;
        meshes.push(format!(
            r#"{{
    "name": "{}",
    "primitives": [{{
      "attributes": {{ "POSITION": {first}, "NORMAL": {} }},
      "indices": {},
      "material": {}
    }}]
  }}"#,
            part.name,
            first + 1,
            first + 2,
            part.material,
        ));
    }

    // `metallicFactor` is written out for the reason `apps/viewer/src/fixture.rs`
    // and `crates/crcbl/tests/gltf_e2e.rs` both give: glTF's default material is
    // a fully rough *conductor*, which has no diffuse lobe, so a document that
    // left it out would arrive nearly black under the demo's one light.
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0", "generator": "crcbl viewer demo" }},
  "scene": 0,
  "scenes": [{{ "name": "demo", "nodes": [0, 1, 2] }}],
  "nodes": [{}],
  "meshes": [{}],
  "materials": [
    {{
      "name": "crate-paint",
      "pbrMetallicRoughness": {{
        "baseColorFactor": [0.82, 0.46, 0.18, 1.0],
        "metallicFactor": 0.0,
        "roughnessFactor": 0.65
      }}
    }},
    {{
      "name": "ground-slab",
      "pbrMetallicRoughness": {{
        "baseColorFactor": [0.24, 0.29, 0.36, 1.0],
        "metallicFactor": 0.0,
        "roughnessFactor": 0.95
      }}
    }}
  ],
  "accessors": [{}],
  "bufferViews": [{}],
  "buffers": [{{ "byteLength": {} }}]
}}"#,
        nodes_json(),
        meshes.join(", "),
        accessors.join(", "),
        views.join(", "),
        bin.len(),
    );
    glb(&json, &bin)
}

/// One drawable part of the document: a mesh of exactly one primitive, and
/// which material it shades with.
///
/// One primitive each because the two parts are the two materials — a second
/// primitive on either would be a distinction the demo has no use for, and the
/// counts on the listing panel would stop being distinct from each other.
struct Part {
    /// What the mesh is called in the document, and therefore on the listing.
    name: &'static str,
    /// Which of the document's materials the primitive names.
    material: usize,
    /// `POSITION`, in the mesh's own space.
    positions: Vec<[f32; 3]>,
    /// `NORMAL`, one per position. Present because the demo is lit by a single
    /// directional light, and a document with no normals is a silhouette.
    normals: Vec<[f32; 3]>,
    /// Triangle indices, two per face, counter-clockwise seen from outside.
    indices: Vec<u16>,
}

/// The crate: a cube with flat-shaded faces.
///
/// Four vertices per face rather than eight for the whole box, because the
/// normals differ per face and a shared corner can only carry one of them.
fn crate_part() -> Part {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for face in &FACES {
        let base = u16::try_from(positions.len()).expect("a cube's vertex count fits a u16");
        positions.extend_from_slice(&corners(face, CRATE_HALF));
        normals.extend_from_slice(&[face.normal; 4]);
        indices.extend_from_slice(&quad_indices(base));
    }
    Part {
        name: "crate",
        material: 0,
        positions,
        normals,
        indices,
    }
}

/// The ground: one quad facing up, for the crates to stand on.
///
/// It is what gives the demo a sense of scale and the orbit camera something
/// that is not the subject; the grid the viewer draws under everything is a
/// debug overlay, not geometry, so a document with no floor in it reads as a
/// box hanging in space.
fn ground_part() -> Part {
    Part {
        name: "ground",
        material: 1,
        positions: corners(&UP, GROUND_HALF).to_vec(),
        normals: vec![UP.normal; 4],
        indices: quad_indices(0).to_vec(),
    }
}

/// The face's four corners on a box of half-extents `half`, counter-clockwise
/// seen from outside.
///
/// Each corner is the face's centre plus half a step along each in-plane axis,
/// and because `normal`, `u` and `v` are distinct unit axes the three
/// contributions land in three different components — which is why one
/// component-wise expression covers all six faces.
fn corners(face: &Face, half: [f32; 3]) -> [[f32; 3]; 4] {
    /// The corner order: the two in-plane signs, swept counter-clockwise.
    const SIGNS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];

    SIGNS.map(|(su, sv)| {
        array::from_fn(|axis| {
            (face.normal[axis] + su * face.u[axis] + sv * face.v[axis]) * half[axis]
        })
    })
}

/// Two triangles over the four corners [`corners`] produced, starting at
/// vertex `base`.
fn quad_indices(base: u16) -> [u16; 6] {
    [base, base + 1, base + 2, base, base + 2, base + 3]
}

/// The document's three nodes: the ground, and the crate at two placements.
///
/// Every scale here is uniform, and that is not incidental. An unequally scaled
/// instance is one `crcbl::scene::gltf_render::build_render_scene` reports a
/// `scale` skip for — a scaled cone is no longer a cone, so the mesh path falls
/// back to culling it by its bounding sphere — and the demo is the one document
/// in this application that should arrive with nothing on the listing's skip
/// list.
fn nodes_json() -> String {
    // A rotation about `Y` is the quaternion `(0, sin(θ/2), 0, cos(θ/2))`, in
    // glTF's `[x, y, z, w]` order.
    let (sin, cos) = (CRATE_YAW_DEGREES.to_radians() * 0.5).sin_cos();
    format!(
        r#"
    {{ "name": "ground", "mesh": 1, "translation": [0.0, -0.5, 0.0] }},
    {{ "name": "crate", "mesh": 0, "translation": [-0.7, 0.0, 0.0], "rotation": [0.0, {sin}, 0.0, {cos}] }},
    {{ "name": "crate.far", "mesh": 0, "translation": [1.0, -0.2, -1.1], "scale": [0.6, 0.6, 0.6] }}
  "#
    )
}

/// The component-wise minimum and maximum of `positions`.
///
/// glTF requires these on a `POSITION` accessor, and this workspace's importer
/// is one of the readers that does not need them — `crates/crcbl-scene/src/gltf_check.rs`
/// says so, and [`crate::model::world_bounds`] recomputes the box from the
/// positions themselves. They are written anyway because the document is a glTF
/// document before it is this application's input: anything else that opens it
/// may lean on them, and a required field left out is a file that is wrong in a
/// way nothing here would ever report.
fn position_bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let (&first, rest) = positions
        .split_first()
        .expect("every part of the demo has vertices");
    rest.iter().fold((first, first), |(min, max), corner| {
        (
            array::from_fn(|axis| min[axis].min(corner[axis])),
            array::from_fn(|axis| max[axis].max(corner[axis])),
        )
    })
}

/// Where a buffer view starts and how long it is, in bytes.
struct View {
    /// Offset from the start of the BIN chunk.
    offset: usize,
    /// Length of the data itself, which is not the padding that may follow it.
    length: usize,
}

/// Appends `values` to `bin` as little-endian `f32`, and reports the view now
/// holding them.
fn push_vec3(bin: &mut Vec<u8>, values: &[[f32; 3]]) -> View {
    let offset = bin.len();
    for value in values.iter().flatten() {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let length = bin.len() - offset;
    pad(bin);
    View { offset, length }
}

/// Appends `values` to `bin` as little-endian `u16`, and reports the view now
/// holding them.
fn push_u16(bin: &mut Vec<u8>, values: &[u16]) -> View {
    let offset = bin.len();
    for value in values {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let length = bin.len() - offset;
    pad(bin);
    View { offset, length }
}

/// Pads `bin` out to a four-byte boundary.
///
/// glTF requires an accessor's byte offset to be a multiple of its component
/// size, so a section of `u16` indices could otherwise leave the section after
/// it starting two bytes into an `f32`.
fn pad(bin: &mut Vec<u8>) {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
}

/// One entry of the document's `bufferViews` array.
fn view_json(view: &View) -> String {
    format!(
        r#"{{ "buffer": 0, "byteOffset": {}, "byteLength": {} }}"#,
        view.offset, view.length
    )
}

/// One JSON array of three numbers.
fn vec3_json(value: [f32; 3]) -> String {
    format!("[{}, {}, {}]", value[0], value[1], value[2])
}

/// A `.glb` container around `json` and `bin`, padded as the format requires:
/// the `JSON` chunk to a multiple of four with spaces, the `BIN` chunk with
/// zeroes.
///
/// **Nearly the same function as `apps/viewer/src/fixture.rs`'s `glb`, and
/// deliberately a second copy.** That one is `#[cfg(test)]` and describes the
/// documents this crate's tests assert on; this one ships in the wasm module.
/// Sharing them would mean either compiling a test fixture into the binary or
/// giving the demo a dependency on a module that exists to be edited whenever a
/// test needs a differently broken file — and a container writer that a test
/// can change is one the demo cannot rely on. They duplicate shape, not
/// knowledge: the shape is the `.glb` specification's, which does not move.
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
        .expect("a demo document under 4 GiB")
        .to_le_bytes()
}

/// The key the generated document is filed under.
///
/// It names nothing on any disk and never reaches one: it exists because
/// [`crcbl::scene::import_gltf`] reads through an asset source *by key*, and
/// because it is what an error would name if this document ever failed to load.
/// Spelled like the file it stands in for rather than like an id, so a message
/// about it reads the way every other one this application prints does.
pub const DEMO_KEY: &str = "demo.glb";

/// The demo document, read and converted the way any other one is.
///
/// [`demo_glb`] is the bytes; this is what [`crate::model::load_from`] makes of
/// them. **Not a shortcut around the loader** — the browser demo goes through
/// the same import, the same non-finite scan and the same framing box as a file
/// a person hands the native viewer, because a demo that bypassed them would
/// prove nothing about the path a real document takes.
///
/// Native code compiles this too, though only the browser build calls it. That
/// is deliberate: `crate::web` is `wasm32`-only and nothing in it can be tested
/// on the machine that builds it, so the one part worth a test — that these
/// bytes really do survive the loader — lives here where a test can reach it.
///
/// # Errors
///
/// [`LoadError`](crate::model::LoadError) if the generated document is not one
/// the importer accepts. That is a bug in this module rather than anything a
/// visitor did, and `the_demo_document_loads_through_the_real_loader` below is
/// what holds it.
pub fn demo_document() -> Result<crate::model::Model, crate::model::LoadError> {
    let key = std::path::Path::new(DEMO_KEY);
    let mut source = crcbl::assets::MemorySource::new();
    source
        .insert(key, demo_glb())
        .map_err(|why| crate::model::LoadError::Storage {
            path: key.to_path_buf(),
            why,
        })?;
    // The key twice: it is both what the source is asked for and what an error
    // names, because a document that never had a path has no better answer.
    crate::model::load_from(&source, key, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::assets::MemorySource;

    /// The browser demo's whole loading path, run on a machine that can test it.
    ///
    /// `crate::web` calls [`demo_document`] and treats a failure as impossible;
    /// this is what earns that. It asserts the document arrives with the
    /// geometry the module claims to build, so a change here that still parses
    /// but loses a node cannot pass.
    #[test]
    fn the_demo_document_loads_through_the_real_loader() {
        let model = demo_document().expect("the generated document loads");
        assert_eq!(
            model.render.instances.len(),
            3,
            "the ground and the two crates are what this document places"
        );
        assert!(
            model.render.skipped.is_empty(),
            "the conversion skipped something: {:?}",
            model.render.skipped
        );
        assert!(
            model.bounds.half_extent().min_element() > 0.0,
            "a document with no extent on some axis cannot be framed: {:?}",
            model.bounds
        );
    }
    use crcbl::scene::{GltfScene, import_gltf};
    use std::path::Path;

    /// What the browser calls the document. Nothing resolves it against a
    /// directory — it is the name the file is reported by, and the name the
    /// memory source files it under.
    const KEY: &str = "demo.glb";

    /// The demo document, read back through the seam the browser reads it
    /// through.
    ///
    /// Not through [`crate::model::load_from`]: this has to be the *importer's*
    /// verdict on the bytes, and `load_from` adds refusals of its own that would
    /// mask which half was wrong.
    fn imported() -> GltfScene {
        let mut source = MemorySource::new();
        source
            .insert(Path::new(KEY), demo_glb())
            .expect("the demo's key is a legal asset key");
        import_gltf(&source, Path::new(KEY)).expect("the demo document imports")
    }

    /// **The generated bytes are a document the real importer accepts.**
    ///
    /// Through `MemorySource` and `import_gltf` themselves rather than a parse
    /// written here, because the claim is not that the container is plausible —
    /// it is that the one reader that will ever open it agrees. Every index and
    /// every span in the file is checked on the way through
    /// `crates/crcbl-scene/src/gltf_check.rs`, so an accessor that overran its
    /// view or a primitive naming a material that is not there fails here.
    #[test]
    fn the_demo_document_imports_out_of_a_memory_source() {
        let scene = imported();

        for mesh in scene.meshes() {
            for primitive in mesh.primitives() {
                assert!(
                    !primitive.positions().is_empty(),
                    "{:?} has a primitive with no vertices",
                    mesh.name(),
                );
                assert_eq!(
                    primitive.normals().len(),
                    primitive.positions().len(),
                    "{:?} is missing normals, so the light would have nothing to shade",
                    mesh.name(),
                );
                assert!(
                    !primitive.indices().is_empty() && primitive.indices().len() % 3 == 0,
                    "{:?} does not hold whole triangles: {} indices",
                    mesh.name(),
                    primitive.indices().len(),
                );
                assert!(
                    primitive
                        .positions()
                        .iter()
                        .flatten()
                        .all(|value| value.is_finite()),
                    "{:?} holds a position that is not finite",
                    mesh.name(),
                );
            }
        }
    }

    /// **The document can be framed on.** `OrbitCamera::frame` asserts on
    /// bounds that are not finite, and a box with no extent on some axis is one
    /// the camera cannot find a distance for — so the demo, which nobody gets to
    /// choose not to open, has to be finite and three-dimensional.
    ///
    /// The per-axis assertion is the one that has teeth: the ground plane is
    /// flat, so a document that lost the crates entirely would still produce a
    /// finite, non-empty box — and no height.
    #[test]
    fn the_demo_documents_world_box_is_finite_and_has_depth_on_every_axis() {
        let scene = imported();
        let bounds = crate::model::world_bounds(&scene).expect("the demo places geometry");

        assert!(
            bounds.min.is_finite() && bounds.max.is_finite(),
            "{bounds:?}"
        );
        let half = bounds.half_extent();
        for (axis, extent) in [("x", half.x), ("y", half.y), ("z", half.z)] {
            assert!(
                extent > 0.0,
                "the demo has no extent on {axis}: half-extent {half:?}",
            );
        }
    }

    /// **The listing panel has more than one row to draw, on every list it
    /// draws.** Asserted on the imported scene rather than on the JSON this
    /// module wrote, because a count that is right in the text and wrong after
    /// the import — a node the scene does not reference, a mesh nothing draws —
    /// is exactly the difference that matters to what a visitor sees.
    #[test]
    fn the_demo_document_carries_two_materials_and_more_than_one_node_and_mesh() {
        let scene = imported();

        assert_eq!(scene.meshes().len(), 2, "the crate and the ground");
        assert_eq!(scene.nodes().len(), 3, "the ground and two crates");
        assert_eq!(
            scene.instances().len(),
            3,
            "every node draws, so every one is an instance",
        );

        let materials = scene.materials();
        assert_eq!(materials.len(), 2, "the crate paint and the ground slab");
        assert_ne!(
            materials[0].base_color, materials[1].base_color,
            "two materials of the same colour are one material as far as a viewer is concerned",
        );
        assert!(
            materials
                .iter()
                .all(|material| material.base_color.iter().all(|c| c.is_finite())),
            "{materials:?}",
        );
    }
}
