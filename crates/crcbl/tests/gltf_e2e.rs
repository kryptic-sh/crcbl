//! A glTF file, through the importer and the bridge, drawn on a real GPU.
//!
//! The win condition for `crcbl_scene::gltf_render`: a `.glb` written to disk
//! becomes pixels whose colours come from *its* texture, in the places *its*
//! node hierarchy puts them. Until this existed nothing in the workspace turned
//! an imported document into a `SceneDesc`, so no glTF had ever reached a frame
//! and a conversion test could only have compared one host-side structure to
//! another.
//!
//! # What the frame proves, and how it could be wrong
//!
//! The document is a one-metre quad whose base-colour texture is four texels —
//! red, green, blue and yellow, one per corner — under a nested node hierarchy:
//! a parent that translates and turns a quarter-turn about `Z`, and a child that
//! translates again. Every step of the conversion is load-bearing in the
//! resulting picture, so each has a way to fail visibly:
//!
//! * **The texture reaches the page.** A material whose image was dropped keeps
//!   `GpuMaterial::NO_PAGE` and multiplies by `1.0` — the frame would be one
//!   flat colour and the four hue assertions would fail together.
//! * **The row names the right layer.** A row naming `NO_PAGE` is the same flat
//!   white; a row pointed past the end of the page is refused by `with_scene`
//!   before a device object exists.
//! * **The UVs survive.** Without `TEXCOORD_0` every vertex samples one texel and
//!   the quad is a single hue.
//! * **The hierarchy composes.** The parent's rotation carries the corners a
//!   quarter-turn round, so the four hues land in quadrants that identity — a
//!   dropped or unapplied parent transform — does not produce. Drop the
//!   translation instead and the quad leaves the camera's window entirely.
//!
//! # And a second document, for the second page an importer fills
//!
//! `an_imported_emissive_texture_lights_the_half_of_the_quad_it_covers` draws
//! the same quad under a different material: a black base colour, an
//! `emissiveFactor`, and an `emissiveTexture` that is black over half the
//! surface and white over the other. Every light in that frame is off, so the
//! picture is the emission and nothing else, and the two halves reading apart
//! is the claim that the document's own image became a page layer and that the
//! row points at it. `crates/crcbl/tests/mesh_e2e/emissive_page.rs` already
//! measures what the *shader* does with such a layer; what only a file can
//! prove is that the importer put one there.
//!
//! `#[ignore]` like `render_e2e.rs` and `tiling_e2e.rs`: it needs a real GPU,
//! which `CRCBL_GPU` names and `tests/run-gltf-e2e.sh` supplies. It commits no
//! golden image — the claim is which hue is in which quadrant, not a pixel-exact
//! reference, so it survives the driver-to-driver colour noise a golden would
//! have to tolerate.

#![cfg(all(not(target_arch = "wasm32"), feature = "scene"))]

use std::path::Path;

use crcbl::adapter::{ADAPTER_ENV_VAR, device_type_from_name};
use crcbl::backend::{BACKEND_ENV_VAR, GpuBackend};
use crcbl::hal::Format;
use crcbl::math::Vec3;
use crcbl::render::{
    Antialiasing, Camera, DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer,
    Projection, RenderEffects,
};
use crcbl::scene::gltf_render::build_render_scene;
use crcbl::screenshot::{ForwardScene, OffscreenSetup};
use crcbl_assets::DirSource;
use crcbl_golden::{ChannelOrder, Image};

/// What this binary calls itself in the lines [`Offscreen`] prints.
///
/// Read by `tests/offscreen/verdict.rs`, which is shared with `render_e2e.rs`
/// and `tiling_e2e.rs` and therefore cannot name any of them.
const SUITE: &str = "crcbl gltf e2e";

// The teardown, out of `tests/offscreen/` rather than in here, because the other
// two suites tear the same fixture down and a second copy is a second place a
// fix has to land. That directory holds no `main.rs`, so Cargo builds no target
// of its own from it.
#[path = "offscreen/verdict.rs"]
mod verdict;

use verdict::Offscreen;

/// The square offscreen frame the quad is drawn into, in pixels.
///
/// Even, so the four quadrants are the same size, and large enough that the
/// patch each assertion samples is far from every texel boundary — the one place
/// two rasterisers can land on opposite sides of an interpolated UV.
const EDGE: u32 = 256;

/// How much of the quad the orthographic window frames, as a fraction of its
/// one-metre side.
///
/// Under one, so the window sits strictly inside the face and no quadrant runs
/// off its edge onto the clear colour.
const FRAME_FRACTION: f32 = 0.8;

/// Where the quad's centre ends up once the hierarchy is composed, in metres.
///
/// `T(2, 3, 0) · Rz(90°) · T(0, -1, 0)` moves the origin to
/// `(2, 3, 0) + Rz(90°) · (0, -1, 0)` = `(2, 3, 0) + (1, 0, 0)`. The camera looks
/// at exactly this, so a conversion that lost either translation — or applied
/// them in the wrong order — frames empty space.
const QUAD_CENTRE: Vec3 = Vec3::new(3.0, 3.0, 0.0);

/// The four texels of the document's base-colour image, RGBA8 row-major.
///
/// Four saturated hues rather than four greys, because the assertion each one
/// carries is "this channel dominates" — a claim a tonemap and an unknown
/// driver's colour handling cannot move, where an exact value would need a
/// tolerance nobody could justify.
const TEXELS: [u8; 16] = [
    0xFF, 0x00, 0x00, 0xFF, // (0, 0) red
    0x00, 0xFF, 0x00, 0xFF, // (1, 0) green
    0x00, 0x00, 0xFF, 0xFF, // (0, 1) blue
    0xFF, 0xFF, 0x00, 0xFF, // (1, 1) yellow
];

/// Which channels a hue is made of: `true` where the channel is at full and
/// `false` where it is at zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hue {
    name: &'static str,
    channels: [bool; 3],
}

const RED: Hue = Hue {
    name: "red",
    channels: [true, false, false],
};
const GREEN: Hue = Hue {
    name: "green",
    channels: [false, true, false],
};
const BLUE: Hue = Hue {
    name: "blue",
    channels: [false, false, true],
};
const YELLOW: Hue = Hue {
    name: "yellow",
    channels: [true, true, false],
};

/// The nested hierarchy the base-colour document is drawn under: a parent that
/// translates and turns a quarter-turn about `Z`, and a child that translates
/// again. [`QUAD_CENTRE`] is where the two put the quad.
const PIVOTED_NODES: &str = r#"[
    {
      "name": "pivot",
      "translation": [2.0, 3.0, 0.0],
      "rotation": [0.0, 0.0, 0.70710678, 0.70710678],
      "children": [1]
    },
    { "name": "panel", "mesh": 0, "translation": [0.0, -1.0, 0.0] }
  ]"#;

/// One node at the origin, which is what the emissive document is drawn under:
/// that test's claim is about a texture's two halves, and a rotation would only
/// move which half of the frame each lands in.
const FLAT_NODES: &str = r#"[{ "name": "panel", "mesh": 0 }]"#;

/// The base-colour document's material.
///
/// `metallicFactor` is written out as zero, and it is load-bearing: glTF
/// defaults a material to a *fully rough conductor*, and a conductor has no
/// diffuse lobe at all — the base colour would barely reach the frame. The
/// base-colour factor is white so the texel arrives undiluted.
const BASE_COLOUR_MATERIAL: &str = r#"{
    "name": "swatches",
    "pbrMetallicRoughness": {
      "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
      "metallicFactor": 0.0,
      "roughnessFactor": 1.0,
      "baseColorTexture": { "index": 0 }
    }
  }"#;

/// The `emissiveFactor` the emissive document writes.
///
/// Three descending channels rather than white, so the lit half carries
/// evidence that the *factor* is in the product as well as the texel: a shader
/// or an importer that emitted the texture alone would read the same in all
/// three.
const EMISSIVE_FACTOR: [f32; 3] = [1.0, 0.5, 0.25];

/// The emissive document's material: a **black** base colour under
/// [`EMISSIVE_FACTOR`] and an `emissiveTexture`.
///
/// Black because the frame it is read in has every light off, and a base colour
/// that could pick up so much as an ambient term would put a number in the dark
/// half that this page did not emit.
fn emissive_material() -> String {
    format!(
        r#"{{
    "name": "lamp",
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.0, 0.0, 0.0, 1.0],
      "metallicFactor": 0.0,
      "roughnessFactor": 1.0
    }},
    "emissiveFactor": [{}, {}, {}],
    "emissiveTexture": {{ "index": 0 }}
  }}"#,
        EMISSIVE_FACTOR[0], EMISSIVE_FACTOR[1], EMISSIVE_FACTOR[2]
    )
}

/// The side of the emissive document's image, in texels.
///
/// Wider than the two columns the split needs, and that is what the reading
/// depends on: [`quadrant`] averages a patch a *quarter* of the frame in, and a
/// two-texel image puts that patch inside the bilinear ramp between its two
/// texels — the black half came back at a third of full when this was `2`. At
/// eight, the ramp is one texel wide either side of the seam at `u = 0.5` and
/// both patches sit whole inside a solid run.
const EMISSIVE_SIDE: u32 = 8;

/// The emissive document's image, RGBA8 row-major: the left half black and the
/// right half white, [`EMISSIVE_SIDE`] texels a side.
///
/// A column split rather than four hues, because the claim is that the *page*
/// decides where a surface emits — one row of the material table, one quad, and
/// two readings that can only differ by the texel under them. `u` grows with
/// world `+X` through the UVs above, so the black half is the left of the frame.
fn emissive_texels() -> Vec<u8> {
    (0..EMISSIVE_SIDE * EMISSIVE_SIDE)
        .flat_map(|index| {
            if index % EMISSIVE_SIDE < EMISSIVE_SIDE / 2 {
                [0x00, 0x00, 0x00, 0xFF]
            } else {
                [0xFF; 4]
            }
        })
        .collect()
}

/// The base-colour document: [`TEXELS`] under [`PIVOTED_NODES`], which is what
/// `an_imported_gltf_draws_its_own_texture_where_its_own_hierarchy_puts_it`
/// reads.
fn quad_glb() -> Vec<u8> {
    quad_document(
        &png_bytes(2, 2, &TEXELS),
        PIVOTED_NODES,
        BASE_COLOUR_MATERIAL,
    )
}

/// The emissive document: [`emissive_texels`] under [`FLAT_NODES`], which is
/// what `an_imported_emissive_texture_lights_the_half_of_the_quad_it_covers`
/// reads.
fn emissive_quad_glb() -> Vec<u8> {
    quad_document(
        &png_bytes(EMISSIVE_SIDE, EMISSIVE_SIDE, &emissive_texels()),
        FLAT_NODES,
        &emissive_material(),
    )
}

/// The document, as the bytes of a `.glb`: the quad, `image` in a `bufferView`,
/// `nodes` as the whole of its scene graph and `material` as its one material.
///
/// Assembled here rather than vendored, for the reason
/// `crates/crcbl-scene/src/gltf_fixture.rs` gives about its own fixtures: a
/// binary blob is a fixture nobody reviewing a change can read, and every number
/// this test asserts on is a number written out below.
///
/// Parameterised over those three because the two documents this file draws
/// differ in exactly them and in nothing else — the geometry, the accessors and
/// the `bufferView` arithmetic below are one copy rather than two that drift.
fn quad_document(image: &[u8], nodes: &str, material: &str) -> Vec<u8> {
    // A unit quad in the `XY` plane facing `+Z`, and the texture coordinates
    // that put one texel of a 2×2 image in each corner of it. glTF's `UV` origin
    // is the image's **top-left** and `v` grows downward, so the corner at
    // `+Y` — the top of the quad, seen from `+Z` — is `v = 0`.
    let positions: [[f32; 3]; 4] = [
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ];
    let normals = [[0.0f32, 0.0, 1.0]; 4];
    let uvs: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    // Counter-clockwise seen from `+Z`, which is the front face.
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];

    let mut bin = Vec::new();
    let push_f32 = |bin: &mut Vec<u8>, values: &[f32]| {
        for value in values {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    };
    for position in positions {
        push_f32(&mut bin, &position);
    }
    for normal in normals {
        push_f32(&mut bin, &normal);
    }
    for uv in uvs {
        push_f32(&mut bin, &uv);
    }
    let index_offset = bin.len();
    for index in indices {
        bin.extend_from_slice(&index.to_le_bytes());
    }
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
  "nodes": {nodes},
  "meshes": [{{
    "name": "panel",
    "primitives": [{{
      "attributes": {{ "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }},
      "indices": 3,
      "material": 0
    }}]
  }}],
  "materials": [{material}],
  "textures": [{{ "source": 0 }}],
  "images": [{{ "name": "swatches", "bufferView": 4, "mimeType": "image/png" }}],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC2" }},
    {{ "bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR" }}
  ],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 32 }},
    {{ "buffer": 0, "byteOffset": {index_offset}, "byteLength": 12 }},
    {{ "buffer": 0, "byteOffset": {image_offset}, "byteLength": {} }}
  ],
  "buffers": [{{ "byteLength": {} }}]
}}"#,
        image.len(),
        bin.len(),
    );
    glb(&json, &bin)
}

/// `pixels` as a `width`×`height` RGBA8 PNG.
fn png_bytes(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("the header is well formed");
        writer
            .write_image_data(pixels)
            .expect("the pixels match the header");
    }
    bytes
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
    out.extend_from_slice(
        &u32::try_from(total)
            .expect("a fixture under 4 GiB")
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(json_chunk.len())
            .expect("a fixture under 4 GiB")
            .to_le_bytes(),
    );
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_chunk);
    out.extend_from_slice(
        &u32::try_from(bin_chunk.len())
            .expect("a fixture under 4 GiB")
            .to_le_bytes(),
    );
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin_chunk);
    out
}

/// **The run is the run it says it is**: the backend and the adapter the frame
/// was drawn on are the ones the environment asked for, and both are announced.
///
/// The same pair `render_e2e.rs` and `tiling_e2e.rs` make. The printed line is
/// load-bearing outside this file — `run-gltf-e2e.sh` greps it, fails when it is
/// missing, and compares the pin it exported against the one that arrived, which
/// is the one failure this process cannot see for itself: an unset pin and a pin
/// that never reached the process are the same thing from in here.
fn assert_pins_arrived(setup: &OffscreenSetup) {
    let backend = setup.backend();
    let adapter = setup.adapter();
    let requested_adapter = crcbl::adapter::pin();
    eprintln!(
        "crcbl gltf e2e: device on adapter {id} {name:?} type={kind:?} ({ADAPTER_ENV_VAR}={pin})",
        id = adapter.id.0,
        name = adapter.name,
        kind = adapter.device_type,
        pin = requested_adapter.as_deref().unwrap_or("<unset>"),
    );

    if let Ok(requested) = std::env::var(BACKEND_ENV_VAR) {
        let opened = GpuBackend::from_name(&backend.to_string())
            .expect("every backend the registry can open has a GpuBackend spelling");
        assert_eq!(
            Some(opened),
            GpuBackend::from_name(&requested),
            "{BACKEND_ENV_VAR}={requested} was asked for and {backend} drew the frame"
        );
    }
    if let Some(requested) = requested_adapter.as_deref() {
        let want = device_type_from_name(requested)
            .unwrap_or_else(|| panic!("{ADAPTER_ENV_VAR}={requested} is not a device class"));
        assert_eq!(
            adapter.device_type, want,
            "{ADAPTER_ENV_VAR}={requested} was asked for and adapter {} ({:?}) drew the frame",
            adapter.name, adapter.device_type
        );
    }
}

/// Imports `document` through a real [`DirSource`] over a real directory,
/// converts it, draws one frame of it, and hands back the pixels.
///
/// The source is the production one rather than a mock, so the key rule the
/// document's own `bufferView` image has to satisfy is the one that will apply
/// to a file a user opens.
///
/// `centre` is where the composed hierarchy puts the quad, which the camera
/// looks straight at; `sun` is the light on it; and `effects` is
/// [`None`] for the renderer's own stack — the frame the engine draws by
/// default — or a request for a frame with the passes that could add a term of
/// their own turned off.
fn draw_the_imported_quad(
    document: &[u8],
    centre: Vec3,
    sun: DirectionalLight,
    effects: Option<EffectRequest>,
) -> Image {
    // A logger before anything opens, for `render_e2e.rs`'s reason: without one
    // every `log::info!` a backend emits on the way to a device goes nowhere,
    // and on a runner nobody can log into that output is the whole diagnosis.
    crcbl::core::log::init_logging();

    let dir = tempfile::tempdir().expect("a temporary directory");
    let root = dir.path().join("assets");
    std::fs::create_dir_all(root.join("meshes")).expect("the asset tree");
    std::fs::write(root.join("meshes/panel.glb"), document).expect("the document");

    let key = Path::new("meshes/panel.glb");
    let imported =
        crcbl::scene::import_gltf(&DirSource::at(root), key).expect("the fixture imports");
    let converted = build_render_scene(&imported, key);
    for skip in &converted.skipped {
        eprintln!("crcbl gltf e2e: unexpected skip: {skip}");
    }
    assert_eq!(
        converted.skipped,
        [],
        "nothing about this document is unsupported; a skip here means the frame below \
         is evidence about a fallback rather than about the conversion"
    );
    assert_eq!(
        converted.instances.len(),
        1,
        "one node draws the one primitive"
    );

    let setup = OffscreenSetup::open_forward(EDGE, EDGE, move |device, queue, format| {
        let mut renderer = ForwardRenderer::with_scene(device, queue, format, &converted.scene)?;
        if let Some(effects) = effects {
            renderer.set_effect_request(effects);
        }
        for instance in &converted.instances {
            renderer
                .add_instance(instance)
                .expect("the pool was sized for exactly these instances");
        }
        Ok(ForwardScene {
            camera: Camera {
                // Straight down the quad's normal, so a world metre maps to a
                // fixed span of pixels and a quadrant of the face is a quadrant
                // of the frame.
                eye: centre + Vec3::Z * 3.0,
                target: centre,
                up: Vec3::Y,
                projection: Projection::Orthographic {
                    half_height: 0.5 * FRAME_FRACTION,
                    near: 0.1,
                    far: 10.0,
                },
            },
            sun,
            renderer: Box::new(renderer),
        })
    })
    .unwrap_or_else(|why| panic!("a GPU backend opens for the glTF test: {why}"));
    let mut setup = Offscreen::guard(SUITE, setup);

    assert_pins_arrived(&setup);

    let format = setup.format();
    let ((width, height), pixels) = setup.draw_and_readback().expect("the frame renders");
    // Before a hue is read out of a quadrant: a device lost during the frame,
    // and a specification violation the layer refused, both surface here and
    // nowhere else — so a run that sampled the pixels first would report a wrong
    // colour where the real answer is that the frame was never legal.
    setup.finish();

    let order = match format {
        Format::Bgra8Unorm | Format::Bgra8UnormSrgb => ChannelOrder::Bgra,
        _ => ChannelOrder::Rgba,
    };
    Image::from_readback(width, height, &pixels, order).expect("the readback is one image")
}

/// The average pixel of the patch at the centre of one quadrant.
///
/// A patch rather than a single pixel, so one stray texel — the seam between two
/// swatches landing a pixel off on one driver — cannot decide the result. Its
/// centre is a quarter of the way in on each axis, which is as far from the
/// quad's edge and from the seam between swatches as a sample can be.
fn quadrant(image: &Image, right: bool, bottom: bool) -> [f32; 3] {
    let span = image.width() / 8;
    let centre = |far: bool, extent: u32| if far { extent * 3 / 4 } else { extent / 4 };
    let (cx, cy) = (centre(right, image.width()), centre(bottom, image.height()));

    let mut total = [0.0f32; 3];
    let mut count = 0.0f32;
    for y in cy - span / 2..cy + span / 2 {
        for x in cx - span / 2..cx + span / 2 {
            let pixel = image.pixel(x, y).expect("the patch is inside the frame");
            for channel in 0..3 {
                total[channel] += f32::from(pixel[channel]);
            }
            count += 1.0;
        }
    }
    [total[0] / count, total[1] / count, total[2] / count]
}

/// Whether `pixel` is `hue`: every channel the hue has at full is brighter than
/// every channel it has at zero, by a margin no shading difference could
/// produce.
fn is_hue(pixel: [f32; 3], hue: Hue) -> bool {
    let lit = (0..3).filter(|&channel| hue.channels[channel]);
    let dark = (0..3).filter(|&channel| !hue.channels[channel]);
    let dimmest_lit = lit
        .map(|channel| pixel[channel])
        .fold(f32::INFINITY, f32::min);
    let brightest_dark = dark
        .map(|channel| pixel[channel])
        .fold(f32::NEG_INFINITY, f32::max);
    dimmest_lit > brightest_dark + 40.0
}

/// The four texels of the document's texture land in the four quadrants its node
/// hierarchy puts them in.
///
/// The quarter-turn on the parent node is what makes the mapping a claim rather
/// than a coincidence: the quad's own top-left corner is red, and the rotation
/// carries it to the frame's **left, below the centre**. A conversion that
/// composed no parent transform would put red where green is, and every one of
/// the four assertions below would name a different colour than it found.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-gltf-e2e.sh"]
fn an_imported_gltf_draws_its_own_texture_where_its_own_hierarchy_puts_it() {
    // Straight onto the face and a healthy ambient, so each swatch's own hue is
    // what separates it from its neighbours rather than any shading gradient.
    let image = draw_the_imported_quad(
        &quad_glb(),
        QUAD_CENTRE,
        DirectionalLight {
            direction: Vec3::Z,
            color: Vec3::splat(1.2),
            ambient: Vec3::splat(0.35),
        },
        None,
    );

    // (right, bottom) → the hue the composed transform puts there. The quad's
    // corners before the parent's `Rz(90°)`: red at `(-x, +y)`, green at
    // `(+x, +y)`, blue at `(-x, -y)`, yellow at `(+x, -y)`. The rotation maps
    // `(x, y)` to `(-y, x)`, so red goes to `(-x, -y)`, green to `(-x, +y)`,
    // yellow to `(+x, +y)` and blue to `(+x, -y)`. In frame terms, with `+Y` up:
    let expected = [
        ((false, true), RED),
        ((false, false), GREEN),
        ((true, false), YELLOW),
        ((true, true), BLUE),
    ];

    let mut found = Vec::new();
    for ((right, bottom), hue) in expected {
        let pixel = quadrant(&image, right, bottom);
        found.push(((right, bottom), pixel));
        eprintln!(
            "crcbl gltf e2e: quadrant right={right} bottom={bottom} is \
             ({:.0}, {:.0}, {:.0}), expecting {}",
            pixel[0], pixel[1], pixel[2], hue.name
        );
    }

    // Anti-vacuity first: four identical quadrants would satisfy nothing below
    // for the right reason, and it is the shape an untextured fallback takes —
    // the whole quad white, every quadrant the same. Say so plainly rather than
    // letting four hue assertions each fail with a different message.
    let spread = (0..3)
        .map(|channel| {
            let values: Vec<f32> = found.iter().map(|(_, pixel)| pixel[channel]).collect();
            let low = values.iter().copied().fold(f32::INFINITY, f32::min);
            let high = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            high - low
        })
        .fold(0.0f32, f32::max);
    assert!(
        spread > 40.0,
        "the four quadrants differ by at most {spread:.0} in any channel, so the quad is \
         one flat colour — the texture never reached the page, or the material row names \
         no page at all"
    );

    for ((right, bottom), hue) in expected {
        let pixel = quadrant(&image, right, bottom);
        assert!(
            is_hue(pixel, hue),
            "the quadrant at right={right} bottom={bottom} should be {} and is \
             ({:.0}, {:.0}, {:.0})",
            hue.name,
            pixel[0],
            pixel[1],
            pixel[2],
        );
    }
}

/// **An imported `emissiveTexture` lights the half of the surface it covers and
/// leaves the other half dark**, with every light in the frame off.
///
/// The importer half of topic 43 §2's rung 3, on a
/// device and through a real file: `crates/crcbl/tests/mesh_e2e/emissive_page.rs`
/// already measures what the shader does with an emissive layer, and its layers
/// are authored by hand into a `PageDesc`. What is unproven until here is that a
/// `.glb`'s own `emissiveTexture` becomes that layer and that the row points at
/// it — a conversion that dropped the image keeps `NO_PAGE`, whose texel is
/// `1.0`, and would light **both** halves at the factor.
///
/// The sun is black and the ambient is zero, and the base colour is black
/// besides, so the whole frame is the emission and nothing else. The passes
/// that could put light in the dark half without the page's help — bloom
/// spreading the lit half across the seam, the reflection march, the AA resolve
/// — are turned off, on `mesh_e2e/emissive_page.rs`'s terms.
///
/// # Sweep
///
/// Measured 2026-09-06 before [`DARK_CEILING`] and [`BRIGHT_FLOOR`] were fixed.
/// On radv both dark patches read `[0.0, 0.0, 0.0]` and both lit ones
/// `[255.0, 188.0, 137.0]`; on lavapipe `[0.0, 0.0, 0.0]` and
/// `[255.0, 187.0, 137.0]`. The dark half is *exactly* zero on both, which is
/// what [`EMISSIVE_SIDE`] bought: the patch sits whole inside the black run
/// rather than in the ramp at the seam.
///
/// # Sabotage
///
/// `crcbl_scene::gltf_render::material_rows` no longer writing
/// `emissive_texture: layer(PageKind::Emissive)`, so the row keeps the
/// `NO_PAGE` the importer left on it. Red on radv on 2026-09-06 with
/// `"channel 0 of dark patch 0 read 255.0; the texel there is black, and a row
/// that lost its emissive page would emit the factor across the whole quad"` —
/// and the printed line reading `dark [[255.0, 188.0, 137.0], …]` against a lit
/// half of exactly the same numbers, which is the shape that failure takes.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-gltf-e2e.sh"]
fn an_imported_emissive_texture_lights_the_half_of_the_quad_it_covers() {
    let image = draw_the_imported_quad(
        &emissive_quad_glb(),
        Vec3::ZERO,
        // No light at all, so every term but the emission is exactly zero and a
        // reading is the page's own product.
        DirectionalLight {
            direction: Vec3::Z,
            color: Vec3::ZERO,
            ambient: Vec3::ZERO,
        },
        Some(EffectRequest {
            programmatic: EffectOverride::none()
                .force(Antialiasing::SLOT, Some(false))
                .force(RenderEffects::REFLECTIONS, Some(false))
                .force(RenderEffects::SHADOWS, Some(false))
                .force(RenderEffects::BLOOM, Some(false))
                .force(RenderEffects::AMBIENT_OCCLUSION, Some(false)),
            ..EffectRequest::default()
        }),
    );

    // Both rows of each half, because the split is a column of the image: a
    // conversion that lost the UVs would put one texel over the whole quad, and
    // then all four readings are one value — which fails the two checks below
    // together, since a quad at the black texel misses the floor and one at the
    // white texel misses the ceiling.
    let dark = [
        quadrant(&image, false, false),
        quadrant(&image, false, true),
    ];
    let bright = [quadrant(&image, true, false), quadrant(&image, true, true)];
    eprintln!(
        "crcbl gltf e2e: the emissive split reads dark {dark:?} and bright {bright:?} \
         against a factor of {EMISSIVE_FACTOR:?}"
    );

    for (patch, reading) in dark.iter().enumerate() {
        for (channel, value) in reading.iter().enumerate() {
            assert!(
                *value < DARK_CEILING,
                "channel {channel} of dark patch {patch} read {value:.1}; the texel there \
                 is black, and a row that lost its emissive page would emit the factor \
                 across the whole quad"
            );
        }
    }
    for (patch, reading) in bright.iter().enumerate() {
        assert!(
            reading[0] > BRIGHT_FLOOR,
            "the red channel of lit patch {patch} read {:.1}; the texel there is white \
             and the factor's first channel is {}, so this half has to be lit",
            reading[0],
            EMISSIVE_FACTOR[0]
        );
        assert!(
            reading[0] > reading[1] && reading[1] > reading[2],
            "lit patch {patch} read {reading:?}, which does not descend the way its \
             factor {EMISSIVE_FACTOR:?} does — the row's radiance has to be in the \
             product, not the texel alone"
        );
    }
}

/// The value, on an eight-bit channel of the read-back frame, that the unlit
/// half must stay under.
///
/// Swept on both drivers before it was fixed, where it reads zero on each; a
/// few levels of room rather than zero itself, because a driver whose bilinear
/// rounding differs by a level is not the regression this guards. A row that
/// lost its emissive page reads the lit half's numbers here instead, which is
/// two orders away.
const DARK_CEILING: f32 = 8.0;

/// The value the lit half's red channel must reach.
///
/// Swept on both drivers, where it saturates at 255 — the factor's first
/// channel is one and the texel is white.
const BRIGHT_FLOOR: f32 = 200.0;
