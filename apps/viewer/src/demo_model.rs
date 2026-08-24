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
//! `1` and prove nothing about the panel. Hence two meshes at three drawing
//! nodes with two visibly different materials: the mesh, node, material and
//! instance counts are all distinct from each other and all greater than one,
//! and the arrangement has depth for the orbit camera to turn around.
//!
//! # It has a rig, and the rig is there to be reported
//!
//! [`crcbl::scene::GltfScene`] reads skins and animation clips, and nothing in
//! this engine poses a skeleton yet — so the rig here is read, reported and not
//! played. That is exactly why it is in the *demo* document rather than only in
//! a test fixture: `crate::listing` names the clips and `crate::app`'s `[HUD]`
//! line counts the joints, and over a document with no rig both would report
//! nothing whether the import works or was never written. The browser gate in
//! `web/tools/browser-e2e.mjs` requires the joint count this document declares,
//! which is a check that can only pass because the rig is here.
//!
//! It is a skin over two joint nodes, `JOINTS_0` and `WEIGHTS_0` on the crate's
//! primitive, and one clip that turns the upper joint. **A joint node draws
//! nothing**, so it adds no instance and no vertex: the instance count is still
//! three and the box the camera frames on is still the crates and the ground.

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

/// Which entries of the document's `nodes` array the crate's two joints are.
///
/// Written down because three places have to agree about them: the scene's root
/// list, the skin's `joints`, and the animation channel's target node.
const JOINT_NODES: [usize; 2] = [3, 4];

/// Where the near crate stands, in metres.
///
/// Written down because the crate's node and the joint it hangs from both place
/// it and have to agree — see [`nodes_json`].
const CRATE_AT: [f32; 3] = [-0.7, 0.0, 0.0];

/// Where each joint binds, as a height above the crate mesh's own origin.
///
/// The crate is a one-metre cube centred on its node, so a joint a quarter of a
/// metre below the centre and one a quarter above it each sit in the middle of
/// the half of the box they own — which is what makes [`bindings`] a straight
/// split by the sign of a vertex's `Y`. Neither is zero, so neither inverse
/// bind matrix is an identity: a rig whose matrices were all identities would
/// read the same whether the importer applied them or ignored them.
const JOINT_BIND_Y: [f32; 2] = [-0.25, 0.25];

/// The clip's keyframe times, in seconds. Ascending, which glTF requires and
/// which the `min`/`max` written onto the sampler's input accessor assumes.
const CLIP_TIMES: [f32; 2] = [0.0, 1.0];

/// How far the clip swings the upper joint about `Z`, in degrees.
const LID_SWING_DEGREES: f32 = 45.0;

/// What the document calls its one animation.
///
/// Named at all — glTF does not require an animation to carry a name — because
/// [`crate::listing`] draws clip names, and a document whose only clip had none
/// would show that panel nothing but the stand-in `crate::model` substitutes.
const CLIP_NAME: &str = "lid-swing";

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
/// directional light has two albedos to separate. Then a rig over the crate —
/// a skin, two joint nodes and one clip — for the reason the [module
/// docs](self) give.
///
/// Open it the way `apps/viewer/src/model.rs` documents: file it in a
/// [`MemorySource`](crcbl::assets::MemorySource) under the name it should be
/// reported by, and hand that to
/// [`load_from`](crate::model::load_from).
#[must_use]
pub fn demo_glb() -> Vec<u8> {
    let parts = [crate_part(), ground_part()];

    // The BIN chunk and the two JSON arrays that index into it are built in one
    // pass, because they have to agree: every accessor is pushed beside the view
    // it reads, so the two arrays are the same length and an accessor's own
    // index is `accessors.len()` at the moment it is written. Nothing indexes
    // either array by position — a skinned part carries more accessors than an
    // unskinned one — so each index that the mesh, skin and animation JSON needs
    // is taken as it is made.
    let mut bin = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    let mut meshes = Vec::new();
    for part in &parts {
        let first = accessors.len();
        let (min, max) = position_bounds(&part.positions);
        let positions = push_floats(&mut bin, &part.positions);
        let normals = push_floats(&mut bin, &part.normals);
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

        let mut attributes = format!(r#""POSITION": {first}, "NORMAL": {}"#, first + 1);
        if let Some(rig) = &part.rig {
            let joint_bytes = push_u16(&mut bin, rig.joints.as_flattened());
            let weight_bytes = push_floats(&mut bin, &rig.weights);
            attributes.push_str(&format!(
                r#", "JOINTS_0": {}, "WEIGHTS_0": {}"#,
                accessors.len(),
                accessors.len() + 1,
            ));
            // `UNSIGNED_SHORT`, because the specification's `JOINTS_0` is an
            // unsigned byte or short and never a float.
            accessors.push(format!(
                r#"{{ "bufferView": {}, "componentType": 5123, "count": {}, "type": "VEC4" }}"#,
                views.len(),
                rig.joints.len(),
            ));
            views.push(view_json(&joint_bytes));
            accessors.push(format!(
                r#"{{ "bufferView": {}, "componentType": 5126, "count": {}, "type": "VEC4" }}"#,
                views.len(),
                rig.weights.len(),
            ));
            views.push(view_json(&weight_bytes));
        }

        meshes.push(format!(
            r#"{{
    "name": "{}",
    "primitives": [{{
      "attributes": {{ {attributes} }},
      "indices": {},
      "material": {}
    }}]
  }}"#,
            part.name,
            first + 2,
            part.material,
        ));
    }

    // The rig's own accessors, after both parts' — see the note above the loop
    // for why they are simply the next ones rather than fixed indices.
    let inverse_bind_bytes = push_floats(&mut bin, &inverse_binds());
    let time_bytes = push_floats(&mut bin, &CLIP_TIMES.map(|second| [second]));
    let rotation_bytes = push_floats(&mut bin, &clip_rotations());
    let inverse_binds_at = accessors.len();
    accessors.push(format!(
        r#"{{ "bufferView": {}, "componentType": 5126, "count": {}, "type": "MAT4" }}"#,
        views.len(),
        JOINT_BIND_Y.len(),
    ));
    views.push(view_json(&inverse_bind_bytes));
    // `min` and `max` because the specification requires them on an animation
    // sampler's input, so a player can know a clip's duration without walking
    // its samples. The same reason `POSITION` above carries a pair.
    let times_at = accessors.len();
    accessors.push(format!(
        r#"{{ "bufferView": {}, "componentType": 5126, "count": {}, "type": "SCALAR", "min": [{}], "max": [{}] }}"#,
        views.len(),
        CLIP_TIMES.len(),
        CLIP_TIMES[0],
        CLIP_TIMES[CLIP_TIMES.len() - 1],
    ));
    views.push(view_json(&time_bytes));
    let rotations_at = accessors.len();
    accessors.push(format!(
        r#"{{ "bufferView": {}, "componentType": 5126, "count": {}, "type": "VEC4" }}"#,
        views.len(),
        CLIP_TIMES.len(),
    ));
    views.push(view_json(&rotation_bytes));

    // `metallicFactor` is written out for the reason `apps/viewer/src/fixture.rs`
    // and `crates/crcbl/tests/gltf_e2e.rs` both give: glTF's default material is
    // a fully rough *conductor*, which has no diffuse lobe, so a document that
    // left it out would arrive nearly black under the demo's one light.
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0", "generator": "crcbl viewer demo" }},
  "scene": 0,
  "scenes": [{{ "name": "demo", "nodes": [0, 1, 2, {}] }}],
  "nodes": [{}],
  "meshes": [{}],
  "skins": [{{
    "name": "crate-rig",
    "skeleton": {},
    "joints": [{}, {}],
    "inverseBindMatrices": {inverse_binds_at}
  }}],
  "animations": [{{
    "name": "{CLIP_NAME}",
    "channels": [{{ "sampler": 0, "target": {{ "node": {}, "path": "rotation" }} }}],
    "samplers": [{{ "input": {times_at}, "output": {rotations_at}, "interpolation": "LINEAR" }}]
  }}],
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
        JOINT_NODES[0],
        nodes_json(),
        meshes.join(", "),
        JOINT_NODES[0],
        JOINT_NODES[0],
        JOINT_NODES[1],
        JOINT_NODES[1],
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
    /// The skinning attributes, on the one part that has them.
    ///
    /// `None` on a part the skin does not reach, which writes no `JOINTS_0` and
    /// no `WEIGHTS_0` at all — the two are a pair, and a primitive carrying one
    /// without the other is not a document the specification allows.
    rig: Option<Bindings>,
    /// `POSITION`, in the mesh's own space.
    positions: Vec<[f32; 3]>,
    /// `NORMAL`, one per position. Present because the demo is lit by a single
    /// directional light, and a document with no normals is a silhouette.
    normals: Vec<[f32; 3]>,
    /// Triangle indices, two per face, counter-clockwise seen from outside.
    indices: Vec<u16>,
}

/// A part's `JOINTS_0` and `WEIGHTS_0`, one entry of each per vertex.
///
/// The two together rather than two fields on [`Part`], because they are only
/// ever written as a pair and are the same length by construction — see
/// [`bindings`], which is the only thing that makes one.
struct Bindings {
    /// Which joint of the skin owns each vertex, as indices into the skin's own
    /// `joints` array — not into the document's nodes.
    joints: Vec<[u16; 4]>,
    /// How much of each vertex each of those joints owns.
    weights: Vec<[f32; 4]>,
}

/// Binds `positions` rigidly to the two joints [`JOINT_BIND_Y`] places: the
/// lower half of the box to the lower joint, the upper half to the upper one.
///
/// Rigid, so the leading slot takes the whole vertex and the three past it
/// carry no weight — which is what a rig a person would author for a hinged lid
/// looks like, and it keeps the weights something a reader can check by eye.
fn bindings(positions: &[[f32; 3]]) -> Bindings {
    Bindings {
        joints: positions
            .iter()
            .map(|&[_, y, _]| [u16::from(y >= 0.0), 0, 0, 0])
            .collect(),
        weights: vec![[1.0, 0.0, 0.0, 0.0]; positions.len()],
    }
}

/// Each joint's inverse bind matrix — world to joint-local at bind time —
/// column-major, which is the order glTF stores a `MAT4` in.
///
/// Both joints bind at a pure translation up the `Y` axis, and the inverse of a
/// translation is the opposite translation; the last column is where a
/// column-major matrix keeps one. So joint `n`'s matrix undoes exactly the
/// placement of joint node `n`, and the bind pose reproduces the mesh as it is
/// written — which is the property that makes this a rig rather than two
/// matrices that happen to parse.
fn inverse_binds() -> [[f32; 16]; 2] {
    JOINT_BIND_Y.map(|y| {
        [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, -y, 0.0, 1.0,
        ]
    })
}

/// The upper joint's rotation at each of [`CLIP_TIMES`], as `xyzw` quaternions
/// — the order the format stores them in.
///
/// Identity, then [`LID_SWING_DEGREES`] about `Z`. Built from `sin_cos` of
/// *half* the angle rather than from decimals typed out, because a quaternion
/// carries `θ/2` and that is the step a hand-written one gets wrong silently.
fn clip_rotations() -> [[f32; 4]; 2] {
    let (sin, cos) = (LID_SWING_DEGREES.to_radians() * 0.5).sin_cos();
    [[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, sin, cos]]
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
        rig: Some(bindings(&positions)),
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
        rig: None,
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

/// The document's five nodes: the ground, the crate at two placements, and the
/// two joints the skin binds to.
///
/// The joints are a parent and its child, offset from each other by
/// [`JOINT_BIND_Y`]. Neither draws a mesh, so neither becomes an instance and
/// neither moves the box `crate::model::world_bounds` computes — a joint is a
/// coordinate frame and nothing else. The crate wears the skin; `crate.far`
/// draws the same mesh without one, which is what a document that reuses a
/// rigged mesh as scenery looks like and is legal glTF: it is the reverse — a
/// node naming a skin whose mesh has no bindings — that the specification
/// refuses.
///
/// **The lower joint carries the crate's placement, and that is the whole
/// reason it is written where it is.** glTF is explicit that "the transform of
/// the skinned mesh node MUST be ignored": a renderer that skins draws the
/// crate wherever its *joints* are, and only a renderer that does not skin —
/// which is every renderer in this engine today — draws it at its node. Put the
/// placement on the node alone and those two answers differ, so the crate would
/// jump the day skinning lands. Put it on the joint as well and they agree, at
/// the bind pose, exactly: joint 0's global transform composed with its inverse
/// bind is `T(x, y, z)·R·T(0, -y, 0)`, and a `Y` translation commutes with a `Y`
/// rotation, so that is `T(x, 0, z)·R` — the node's own placement, which is what
/// [`the_bind_pose_puts_the_crate_where_its_node_does`] asserts.
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
    let [x, _, z] = CRATE_AT;
    let [base_y, lid_y] = JOINT_BIND_Y;
    // The upper joint is the lower one's child, so its own translation is the
    // step between them rather than its height.
    let step = lid_y - base_y;
    let lid = JOINT_NODES[1];
    // One string for the turn, written into the crate and into the joint it
    // hangs from, so the two cannot drift apart — see this function's docs for
    // why they have to agree.
    let turn = format!(r#""rotation": [0.0, {sin}, 0.0, {cos}]"#);
    let crate_at = CRATE_AT;
    format!(
        r#"
    {{ "name": "ground", "mesh": 1, "translation": [0.0, -0.5, 0.0] }},
    {{ "name": "crate", "mesh": 0, "skin": 0, "translation": {}, {turn} }},
    {{ "name": "crate.far", "mesh": 0, "translation": [1.0, -0.2, -1.1], "scale": [0.6, 0.6, 0.6] }},
    {{ "name": "joint.base", "children": [{lid}], "translation": {}, {turn} }},
    {{ "name": "joint.lid", "translation": [0.0, {step}, 0.0] }}
  "#,
        vec3_json(crate_at),
        vec3_json([x, base_y, z]),
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
///
/// Generic over the element's width because the document holds five shapes of
/// float accessor — positions and normals, weights and rotations, the bind
/// matrices, and the keyframe times as one-component elements — and they differ
/// only in the number the JSON beside them calls a `type`.
fn push_floats<const N: usize>(bin: &mut Vec<u8>, values: &[[f32; N]]) -> View {
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
    use crcbl::math::{Mat4, Quat, Vec3};
    use crcbl::scene::{GltfSamples, GltfScene, import_gltf};
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
        assert_eq!(
            scene.nodes().len(),
            5,
            "the ground, two crates and the rig's two joints",
        );
        assert_eq!(
            scene.instances().len(),
            3,
            "the joints draw nothing, so only the ground and the crates are instances",
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

    /// **The document really is rigged, and rigged the way this module says.**
    ///
    /// The check the browser gate leans on. `web/tools/browser-e2e.mjs` waits
    /// for `joints: 2` on the viewer's heartbeat, and that number is only ever
    /// right by accident unless the skin, its joint list and its bind matrices
    /// all survive into the imported scene — so this asserts each of them here,
    /// where a failure names which one went, rather than leaving a browser run
    /// to time out on a line that never arrives.
    ///
    /// The bind matrices are checked by what they *do*: joint `n`'s matrix has
    /// to carry joint node `n`'s bind position back to the origin. A pair of
    /// identities parses, imports and reports two joints, and would pass every
    /// assertion here that only counted things.
    #[test]
    fn the_demo_document_carries_the_rig_the_browser_gate_counts() {
        let scene = imported();

        let [skin] = scene.skins() else {
            panic!("the demo declares exactly one skin: {:?}", scene.skins());
        };
        assert_eq!(skin.name(), Some("crate-rig"));
        assert_eq!(skin.joints(), JOINT_NODES, "the skin's own joint nodes");
        assert_eq!(
            skin.skeleton(),
            Some(JOINT_NODES[0]),
            "the lower joint is the root the document names",
        );

        assert_eq!(skin.inverse_binds().len(), JOINT_BIND_Y.len());
        for (joint, (&matrix, &bind_y)) in
            skin.inverse_binds().iter().zip(&JOINT_BIND_Y).enumerate()
        {
            let bound = Vec3::new(0.0, bind_y, 0.0);
            let undone = matrix.transform_point3(bound);
            assert!(
                undone.length() < 1e-6,
                "joint {joint}'s inverse bind leaves its bind pose at {undone:?}, not at the \
                 origin",
            );
            assert_ne!(
                matrix,
                Mat4::IDENTITY,
                "joint {joint}'s inverse bind is an identity, which says nothing",
            );
        }
    }

    /// **Skinning the bind pose puts the crate exactly where its node does.**
    ///
    /// glTF says "the transform of the skinned mesh node MUST be ignored", so a
    /// renderer that skins reads the crate's placement off its *joints* and a
    /// renderer that does not — every renderer in this engine today — reads it
    /// off the node. Those are two answers to one question, and this is the
    /// assertion that they are the same answer: joint 0's global transform
    /// composed with its inverse bind has to equal the crate node's own
    /// placement. Without it the crate moves the day skinning lands, and the
    /// only thing that would notice is a person looking at the demo.
    ///
    /// Checked at both joints, because the crate's vertices hang off both.
    #[test]
    fn the_bind_pose_puts_the_crate_where_its_node_does() {
        let [x, _, z] = CRATE_AT;
        let [base_y, lid_y] = JOINT_BIND_Y;
        let turn = Quat::from_rotation_y(CRATE_YAW_DEGREES.to_radians());
        let node = Mat4::from_rotation_translation(turn, Vec3::from(CRATE_AT));
        // The document's own hierarchy: the lower joint carries the placement,
        // and the upper one is its child a step above it.
        let base = Mat4::from_rotation_translation(turn, Vec3::new(x, base_y, z));
        let lid = base * Mat4::from_translation(Vec3::new(0.0, lid_y - base_y, 0.0));

        for (joint, global) in [base, lid].into_iter().enumerate() {
            let skinned = global * Mat4::from_cols_array(&inverse_binds()[joint]);
            // A cube corner rather than the origin: the origin is where every
            // one of these transforms agrees by construction, so it is the one
            // point that would pass whatever the rotation did.
            let corner = Vec3::new(0.5, 0.5, 0.5);
            let by_skin = skinned.transform_point3(corner);
            let by_node = node.transform_point3(corner);
            assert!(
                by_skin.distance(by_node) < 1e-5,
                "joint {joint} skins the bind pose to {by_skin:?} and the node draws it at \
                 {by_node:?}",
            );
        }
    }

    /// **The crate's node and the joint it hangs from are placed together in
    /// the document**, which is what makes the test above true of the emitted
    /// file rather than only of the arithmetic.
    ///
    /// The one above builds both transforms out of the same constants, so it
    /// says the algebra works and cannot say the JSON uses it. This reads the
    /// artefact: the turn has to be the identical text on both nodes, and the
    /// joint has to stand over the crate — same `X` and `Z`, its own height.
    #[test]
    fn the_crate_and_its_joint_are_placed_together() {
        let nodes = nodes_json();
        let field = |name: &str, field: &str| {
            let at = nodes
                .find(&format!(r#""name": "{name}""#))
                .unwrap_or_else(|| panic!("no node called {name} in {nodes}"));
            let rest = &nodes[at..];
            let end = rest.find('}').expect("a node's object ends");
            let node = &rest[..end];
            let found = node
                .find(&format!(r#""{field}""#))
                .unwrap_or_else(|| panic!("{name} carries no {field}: {node}"));
            let value = &node[found..];
            let close = value.find(']').expect("an array ends");
            value[..=close].to_string()
        };

        assert_eq!(
            field("crate", "rotation"),
            field("joint.base", "rotation"),
            "the joint has to be turned exactly as the crate is",
        );
        let [x, _, z] = CRATE_AT;
        assert_eq!(
            field("joint.base", "translation"),
            format!(r#""translation": {}"#, vec3_json([x, JOINT_BIND_Y[0], z])),
            "the joint has to stand over the crate, at its own height",
        );
    }

    /// **The bindings are on the crate and are the split this module describes**
    /// — the lower half of the box on the lower joint, the upper half on the
    /// upper one.
    ///
    /// Both joints have to be *used*: a document that bound every vertex to
    /// joint 0 still declares two of them, still reports `joints: 2`, and is a
    /// rig with a limb nothing hangs off. And the ground carries no bindings at
    /// all, which is the other half of the claim — `JOINTS_0` written onto
    /// every primitive would make the attribute meaningless as evidence.
    #[test]
    fn the_crates_vertices_are_bound_to_both_joints_and_the_grounds_to_neither() {
        let scene = imported();
        let mesh = scene
            .meshes()
            .iter()
            .find(|mesh| mesh.name() == Some("crate"))
            .expect("the crate mesh");
        let [primitive] = mesh.primitives() else {
            panic!("the crate is one primitive");
        };

        assert_eq!(primitive.joints().len(), primitive.positions().len());
        assert_eq!(primitive.weights().len(), primitive.positions().len());
        for (vertex, (&[_, y, _], &joints)) in primitive
            .positions()
            .iter()
            .zip(primitive.joints())
            .enumerate()
        {
            assert_eq!(
                joints[0],
                u16::from(y >= 0.0),
                "vertex {vertex} at y {y} is on the wrong joint",
            );
        }
        for (vertex, &weights) in primitive.weights().iter().enumerate() {
            let total: f32 = weights.iter().sum();
            assert!(
                (total - 1.0).abs() < 1e-6,
                "vertex {vertex}'s weights come to {total}, so it is partly unbound",
            );
        }
        for joint in 0..u16::try_from(JOINT_BIND_Y.len()).expect("a joint count fits a u16") {
            assert!(
                primitive.joints().iter().any(|slots| slots[0] == joint),
                "no vertex is bound to joint {joint}",
            );
        }

        let ground = scene
            .meshes()
            .iter()
            .find(|mesh| mesh.name() == Some("ground"))
            .expect("the ground mesh");
        for primitive in ground.primitives() {
            assert!(primitive.joints().is_empty(), "the ground wears no skin");
            assert!(primitive.weights().is_empty(), "nor any weights");
        }
    }

    /// **The clip is one rotation channel on the upper joint**, with the
    /// keyframes this module wrote.
    ///
    /// A clip the importer read as a translation channel, or as a channel on
    /// the wrong node, is one that parses and animates the wrong thing — and
    /// the joint count the browser gate reads would not notice either.
    #[test]
    fn the_demo_documents_clip_turns_the_upper_joint() {
        let scene = imported();

        let [clip] = scene.clips() else {
            panic!("the demo declares exactly one clip: {:?}", scene.clips());
        };
        assert_eq!(clip.name(), Some(CLIP_NAME));

        let [channel] = clip.channels() else {
            panic!("the clip is one channel: {:?}", clip.channels());
        };
        assert_eq!(
            channel.node(),
            JOINT_NODES[1],
            "the lid joint is what turns"
        );
        assert_eq!(channel.times(), CLIP_TIMES);
        match channel.samples() {
            GltfSamples::Rotations(rotations) => {
                assert_eq!(rotations.as_slice(), clip_rotations());
            }
            other => panic!("the channel drives a rotation, not {other:?}"),
        }
    }

    /// **The rig reaches the [`Model`](crate::model::Model) the panels read**,
    /// which is the number `web/tools/browser-e2e.mjs` waits for and the text
    /// `crate::listing` draws.
    ///
    /// Asserted through [`demo_document`] rather than through the importer,
    /// because the gap this closes is the one between a document that holds a
    /// rig and an application that reports one: `crate::model::load_from` is
    /// where the imported scene is summarised and then dropped.
    #[test]
    fn the_demo_documents_rig_reaches_the_model_the_panels_read() {
        let model = demo_document().expect("the generated document loads");

        assert_eq!(
            model.rig.joints,
            JOINT_BIND_Y.len(),
            "the joint count on the `[HUD]` line and the browser gate's predicate",
        );
        assert_eq!(model.rig.clips, [CLIP_NAME]);
        assert!(!model.rig.is_empty());
    }
}
