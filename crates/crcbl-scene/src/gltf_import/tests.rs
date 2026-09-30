use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::gltf_fixture::{
    Assets, BASE_COLOR, BIN_CHUNK_BUFFER, CLIP_ROTATIONS, CLIP_TIMES, EXTERNAL_BUFFER,
    IMAGE_TEXELS, INDICES, INVERSE_BIND, JOINTS, NORMALS, POSITIONS, TEX_COORDS, WEIGHTS, glb,
    import_glb, import_glb_bytes, import_glb_bytes_as, import_gltf_text, import_rigged_glb,
    png_bytes, replacing, rigged_json, textured_glb, textured_parts, triangle_bin, triangle_json,
};

/// The row a glTF material with no `pbrMetallicRoughness` block imports as:
/// every factor the specification's own default.
///
/// **Deliberately not [`GpuMaterial::UNTINTED`]**, which it was until the row
/// grew shading factors. glTF defaults `metallicFactor` and
/// `roughnessFactor` to `1.0` — a fully rough conductor — where that row is a
/// dielectric at half roughness, and the importer's job is to report the
/// document. Written out here rather than derived from `UNTINTED`, so a
/// change to the engine's neutral row cannot silently move what a document
/// is claimed to say.
const GLTF_DEFAULT_MATERIAL: GpuMaterial = GpuMaterial {
    base_color: [1.0; 4],
    base_color_texture: GpuMaterial::NO_PAGE,
    metallic: 1.0,
    roughness: 1.0,
    tiling: GpuMaterial::TILING_AUTHORED,
    tile_metres: 1.0,
    // glTF's own default `emissiveFactor`, which is no emission.
    emissive: [0.0; 3],
    normal_texture: GpuMaterial::NO_PAGE,
    // glTF's own default `normalTexture.scale`, which leaves a normal map
    // as it was authored — and which a material with no normal map reports
    // because the specification's default object is what it means.
    normal_scale: 1.0,
    metallic_roughness_occlusion_texture: GpuMaterial::NO_PAGE,
    emissive_texture: GpuMaterial::NO_PAGE,
    // glTF's own default `alphaCutoff`, and no alpha mode set.
    alpha_cutoff: 0.5,
    flags: 0,
    // The dielectric `KHR_materials_ior`'s and `KHR_materials_specular`'s
    // defaults reduce to — an IOR of 1.5 and both factors at one.
    specular_f0: [GpuMaterial::DIELECTRIC_F0; 3],
    specular_f90: 1.0,
};

/// The `animations` array a `KHR_animation_pointer` document carries: a
/// channel whose `target` names a **pointer** and no `node`.
///
/// `gltf_json::animation::Target` makes `node` mandatory, so this is what
/// makes `serde` refuse the whole `Root`. Copied from the shape
/// `AnimatedColorsCube` in the Khronos sample suite uses, which is the
/// document that found this.
const ANIMATION_POINTER: &str = r#""animations": [{
    "samplers": [{ "input": 0, "output": 1, "interpolation": "LINEAR" }],
    "channels": [{
      "sampler": 0,
      "target": {
        "path": "pointer",
        "extensions": {
          "KHR_animation_pointer": { "pointer": "/materials/0/pbrMetallicRoughness/baseColorFactor" }
        }
      }
    }]
  }],
  "extensionsUsed": ["KHR_animation_pointer"],
  "#;

/// **A document is not refused over an animation this importer cannot
/// deserialize.**
///
/// A `KHR_animation_pointer` channel cannot be deserialized at all — `node`
/// is a required field and that extension replaces it — so the failure took
/// the whole document with it. `AnimatedColorsCube` from the Khronos suite
/// lists **nothing** in `extensionsRequired`, so the specification says it
/// has to load, and before `parse_without_animations` it did not. The cost
/// is this document's clips, which is why the repair is loud.
///
/// The geometry is asserted, not just the `Ok`: a repair that dropped the
/// animation and the mesh with it would satisfy "it loads" and be useless.
#[test]
fn an_animation_this_importer_cannot_deserialize_does_not_refuse_the_document() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#""asset": {"#,
        &format!("{ANIMATION_POINTER}\"asset\": {{"),
    );
    let scene = import_glb(&json).expect("the document loads without its animation");

    assert_eq!(scene.meshes().len(), 1, "the mesh went with the animation");
    let primitive = &scene.meshes()[0].primitives()[0];
    assert_eq!(primitive.positions(), POSITIONS);
    assert_eq!(
        primitive.indices(),
        INDICES.map(u32::from),
        "the repaired parse must produce the same geometry as an untouched one",
    );
}

/// **The retry does not turn a malformed document into a silent success.**
///
/// `parse_without_animations` reports `None` for anything that fails for a
/// second reason, so the caller raises the error from the *first* attempt —
/// the accurate one, about the document as written rather than as altered.
#[test]
fn a_document_broken_for_another_reason_is_still_refused_with_its_own_error() {
    // An animation `serde` refuses *and* a `nodes` array that is not an
    // array. Dropping the animations cannot rescue this one.
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#""asset": {"#,
        &format!("{ANIMATION_POINTER}\"asset\": {{"),
    );
    let json = replacing(&json, r#""scenes": [{ "nodes": [0] }],"#, r#""scenes": 7,"#);
    let error = import_glb(&json).expect_err("a malformed document is refused");
    let message = error.to_string();
    assert!(
        !message.contains("animation"),
        "the retry's own failure was reported instead of the document's: {message}",
    );
}

/// Every warning `warn_dropped_features` emitted while importing `json`,
/// under a key no other call has used.
///
/// A fresh key because the extension lines are said once per key per
/// process — see `import_glb_bytes_as`.
fn import_warnings(json: &str) -> Vec<String> {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let key = format!(
        "meshes/warnings-{}.glb",
        CALLS.fetch_add(1, Ordering::Relaxed)
    );
    import_warnings_as(json, &key)
}

/// [`import_warnings`] under a key the caller chose, so a test can import
/// the same asset twice.
fn import_warnings_as(json: &str, key: &str) -> Vec<String> {
    let logs = crcbl_core::log::capture();
    import_glb_bytes_as(&glb(json, Some(&triangle_bin())), key).expect("the fixture imports");
    logs.records()
        .into_iter()
        .filter(|record| record.target.contains("gltf_import"))
        .map(|record| record.message)
        .collect()
}

/// **An asset imported again is not reported again**, and a different asset
/// naming the same extension still is.
///
/// EW imports the same rifle on every range load, and each import used to
/// repeat the same line. The scene's own answer is not deduplicated — a
/// second import of a document is owed the same
/// `unsupported_required_extensions` as the first — only the log line is.
///
/// # Sabotage
///
/// `first_report` returning `true` unconditionally: red with "the second
/// import of the same asset repeated its extension lines".
#[test]
fn an_extension_is_reported_once_per_asset_and_not_once_per_import() {
    let json = with_extensions(r#""KHR_animation_pointer""#, r#""KHR_materials_sheen""#);
    let first = import_warnings_as(&json, "meshes/reported-once.glb");
    let names = |warnings: &[String]| {
        warnings
            .iter()
            .filter(|line| {
                line.contains("KHR_animation_pointer") || line.contains("KHR_materials_sheen")
            })
            .count()
    };
    assert_eq!(
        names(&first),
        2,
        "the first import names both extensions once each: {first:#?}"
    );

    let second = import_warnings_as(&json, "meshes/reported-once.glb");
    assert_eq!(
        names(&second),
        0,
        "the second import of the same asset repeated its extension lines: {second:#?}"
    );

    let other = import_warnings_as(&json, "meshes/reported-elsewhere.glb");
    assert_eq!(
        names(&other),
        2,
        "a different asset naming the same extensions is its own report: {other:#?}"
    );

    let again = import_glb_bytes_as(
        &glb(&json, Some(&triangle_bin())),
        "meshes/reported-once.glb",
    )
    .expect("the fixture imports");
    assert_eq!(
        again.unsupported_required_extensions(),
        ["KHR_materials_sheen"],
        "the scene's own answer must not be deduplicated with the log line"
    );
}

/// **The IOR and specular extensions are implemented, so neither is
/// reported** — the line EW's Mossberg asset drew on every import.
#[test]
fn the_ior_and_specular_extensions_are_not_reported() {
    let names = r#""KHR_materials_ior", "KHR_materials_specular""#;
    let warnings = import_warnings(&with_extensions(names, names));
    assert!(
        !warnings
            .iter()
            .any(|line| line.contains("KHR_materials_ior")
                || line.contains("KHR_materials_specular")),
        "an implemented extension was reported as unsupported: {warnings:#?}",
    );
}

/// **Emissive strength is implemented too, so it is not reported** — it
/// was read into the row's radiance while the list still called it
/// ignored.
#[test]
fn the_emissive_strength_extension_is_not_reported() {
    let name = r#""KHR_materials_emissive_strength""#;
    let warnings = import_warnings(&with_extensions(name, name));
    assert!(
        !warnings
            .iter()
            .any(|line| line.contains("KHR_materials_emissive_strength")),
        "an implemented extension was reported as unsupported: {warnings:#?}",
    );
}

/// The fixture with `extensionsUsed`/`extensionsRequired` spliced in.
fn with_extensions(used: &str, required: &str) -> String {
    replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#""asset": {"#,
        &format!(r#""extensionsUsed": [{used}], "extensionsRequired": [{required}], "asset": {{"#),
    )
}

/// **A required extension this importer cannot honour is named, not
/// swallowed.**
///
/// The viewer's exit criterion asks for the file, the
/// feature and the reason. `SheenWoodLeatherSofa` and three others from the
/// Khronos suite used to load and draw wrong in silence — the only clue was
/// that the picture looked off.
#[test]
fn a_required_extension_the_importer_lacks_is_named_in_a_warning() {
    let warnings = import_warnings(&with_extensions("", r#""KHR_materials_sheen""#));
    let named = warnings
        .iter()
        .find(|line| line.contains("REQUIRES"))
        .unwrap_or_else(|| panic!("no line named the required extension: {warnings:#?}"));
    assert!(
        named.contains("KHR_materials_sheen"),
        "the line does not name the extension: {named}",
    );
}

/// An extension the document itself calls optional gets the quieter line,
/// and is not reported as required — the two say different things about
/// whether what is on screen is the file.
#[test]
fn an_optional_extension_is_reported_separately_from_a_required_one() {
    let warnings = import_warnings(&with_extensions(r#""KHR_animation_pointer""#, ""));
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("ignoring") && line.contains("KHR_animation_pointer")),
        "the optional extension was not reported: {warnings:#?}",
    );
    assert!(
        !warnings.iter().any(|line| line.contains("REQUIRES")),
        "an optional extension was reported as required: {warnings:#?}",
    );
}

/// **An extension the importer *does* implement is not reported at all.**
///
/// The guard that keeps this list honest: without it the warning fires for
/// `MSFT_lod`, which `lod_resolve` reads, and every document using it would
/// carry a line saying its levels were ignored while they were being
/// resolved.
#[test]
fn an_extension_this_importer_implements_is_not_reported() {
    let warnings = import_warnings(&with_extensions(r#""MSFT_lod""#, r#""MSFT_lod""#));
    assert!(
        !warnings
            .iter()
            .any(|line| line.contains("MSFT_lod") || line.contains("REQUIRES")),
        "an implemented extension was reported as unsupported: {warnings:#?}",
    );
}

/// **The required extensions are on the scene, not only in the log.**
///
/// A warning is for the person watching a run go past; this is what a
/// caller reports beside the model and what a test can assert, which is
/// what makes `apps/viewer`'s `shelf.expect` able to say that a model is
/// expected to draw without something it declared it needed. The optional
/// entry in the same document is deliberately absent: the document itself
/// says the picture is right without it.
#[test]
fn the_scene_carries_the_required_extensions_the_importer_lacks() {
    let scene = import_glb(&with_extensions(
        r#""KHR_animation_pointer", "MSFT_lod""#,
        r#""KHR_materials_sheen", "MSFT_lod""#,
    ))
    .expect("the fixture imports");
    assert_eq!(
        scene.unsupported_required_extensions(),
        ["KHR_materials_sheen"],
        "the scene does not name exactly the required extensions this importer lacks",
    );

    let plain = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).expect("the fixture imports");
    assert!(
        plain.unsupported_required_extensions().is_empty(),
        "a document declaring no extension has one",
    );
}

/// An extension in both lists is named once, as required. Listing it twice
/// would read as two different problems with the same document.
#[test]
fn an_extension_in_both_lists_is_named_once() {
    let name = r#""KHR_texture_transform""#;
    let warnings = import_warnings(&with_extensions(name, name));
    let mentions = warnings
        .iter()
        .filter(|line| line.contains("KHR_texture_transform"))
        .count();
    assert_eq!(mentions, 1, "named more than once: {warnings:#?}");
}

/// **A texture whose image an extension supplies is skipped, not refused.**
///
/// `source` has a `serde` default of `u32::MAX` — see
/// `gltf_check::TEXTURE_SOURCE_ABSENT` — so a texture that omits it used to
/// be refused with `texture 0 names image 4294967295`, a sentinel this crate
/// invented reported as though the document had written it.
/// `SheenWoodLeatherSofa` from the Khronos suite was the one model in that
/// suite refused for it.
///
/// The material must survive with **no** texture rather than with a broken
/// one: `gltf::Texture::source` is an `unwrap` on that index and would abort
/// the process.
#[test]
fn a_texture_that_names_no_image_is_skipped_and_its_material_keeps_its_colour() {
    let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
    let json = replacing(
        &base,
        r#""textures": [{ "source": 0 }]"#,
        r#""textures": [{ }]"#,
    );
    let scene = import_glb_bytes(&glb(&json, Some(&bin)))
        .expect("a texture with no image does not refuse the document");

    assert_eq!(
        scene.materials().len(),
        1,
        "the material went with its texture",
    );
    assert!(
        scene.base_color_textures()[0].is_none(),
        "the material kept a texture whose image cannot be read",
    );
}

/// And it says so, naming the count — otherwise a material coming out
/// untextured has no explanation anywhere.
#[test]
fn a_skipped_texture_is_reported() {
    let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
    let json = replacing(
        &base,
        r#""textures": [{ "source": 0 }]"#,
        r#""textures": [{ }]"#,
    );
    let logs = crcbl_core::log::capture();
    import_glb_bytes(&glb(&json, Some(&bin))).expect("the document imports");
    let messages: Vec<String> = logs
        .records()
        .into_iter()
        .filter(|record| record.target.contains("gltf_import"))
        .map(|record| record.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|line| line.contains("1 texture(s) that name no image")),
        "nothing said the texture was skipped: {messages:#?}",
    );
}

/// **An out-of-range source is still refused**, which is the half that must
/// not move: that is a document naming an image it does not have, and
/// letting it through would put an `unwrap` on a real index.
#[test]
fn a_texture_naming_an_image_that_does_not_exist_is_still_refused() {
    let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
    let json = replacing(
        &base,
        r#""textures": [{ "source": 0 }]"#,
        r#""textures": [{ "source": 9 }]"#,
    );
    let error = import_glb_bytes(&glb(&json, Some(&bin)))
        .expect_err("a texture naming a missing image is refused");
    assert!(
        error.to_string().contains("texture 0 names image 9"),
        "the refusal does not name the index: {error}",
    );
}

/// The `TANGENT` [`tangent_glb`] authors, one per vertex of the fixture
/// triangle.
///
/// Every one is a unit vector perpendicular to the fixture's normal —
/// `[0, 0, 1]` on all three vertices — so the frame each makes is
/// orthonormal and survives a `QTangent` round trip to that type's own
/// stated error. Their handedness is not all the same, deliberately: the
/// sign in `w` is the half of the attribute that no amount of geometry can
/// re-derive, so a fixture whose every vertex wrote `+1` would leave
/// exactly that half untested.
pub(crate) const TANGENTS: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 1.0],
    [0.0, 1.0, 0.0, -1.0],
    [1.0, 0.0, 0.0, -1.0],
];

/// The `normalTexture.scale` [`normal_mapped_glb`] authors.
///
/// Deliberately not glTF's own default, so a row that reported the
/// specification's value instead of the document's could not pass for it.
pub(crate) const NORMAL_SCALE: f32 = 0.75;

/// [`triangle_bin`] with [`TANGENTS`] appended, which is what
/// [`tangent_json`]'s fifth buffer view slices.
pub(crate) fn tangent_bin() -> Vec<u8> {
    let mut bytes = triangle_bin();
    // A buffer view is four-byte aligned like any other, and an unaligned
    // one would be this fixture's bug rather than the importer's.
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    for tangent in TANGENTS {
        for component in tangent {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// [`triangle_json`] with a `TANGENT` attribute over [`tangent_bin`].
///
/// `count` is the accessor's own count rather than [`TANGENTS`]' length, so
/// a caller can author a `TANGENT` **shorter** than `POSITION` — the case
/// the length check refuses.
pub(crate) fn tangent_json(count: usize) -> String {
    let bin = tangent_bin();
    let span = TANGENTS.len() * size_of::<[f32; 4]>();
    let offset = bin.len() - span;
    let base = triangle_json(&format!(r#"{{ "byteLength": {} }}"#, bin.len()));
    let with_attribute = replacing(
        &base,
        r#""POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2"#,
        r#""POSITION": 0, "NORMAL": 1, "TANGENT": 4, "TEXCOORD_0": 2"#,
    );
    let with_accessor = replacing(
        &with_attribute,
        r#"{ "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }"#,
        &format!(
            r#"{{ "bufferView": 3, "componentType": 5123, "count": 3, "type": "SCALAR" }},
    {{ "bufferView": 4, "componentType": 5126, "count": {count}, "type": "VEC4" }}"#
        ),
    );
    replacing(
        &with_accessor,
        r#"{ "buffer": 0, "byteOffset": 96, "byteLength": 6 }"#,
        &format!(
            r#"{{ "buffer": 0, "byteOffset": 96, "byteLength": 6 }},
    {{ "buffer": 0, "byteOffset": {offset}, "byteLength": {span} }}"#
        ),
    )
}

/// [`tangent_json`] closed into a `.glb`.
pub(crate) fn tangent_glb(count: usize) -> Vec<u8> {
    glb(&tangent_json(count), Some(&tangent_bin()))
}

/// A one-triangle document whose material carries a `normalTexture` at
/// [`NORMAL_SCALE`] — and **no** `baseColorTexture`.
///
/// The base-colour slot is left empty deliberately. The two pages are
/// indexed separately, so a document that filled both would let a row
/// pointed at the wrong page's layer map still read the right number.
pub(crate) fn normal_mapped_glb() -> Vec<u8> {
    let (json, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
    let json = replacing(
        &json,
        r#""baseColorFactor": [0.25, 0.5, 0.75, 1.0],
      "baseColorTexture": { "index": 0, "texCoord": 0 }
    }"#,
        &format!(
            r#""baseColorFactor": [0.25, 0.5, 0.75, 1.0]
    }},
    "normalTexture": {{ "index": 0, "texCoord": 0, "scale": {NORMAL_SCALE} }}"#
        ),
    );
    glb(&json, Some(&bin))
}

/// The `emissiveFactor` [`emissive_textured_glb`] authors.
///
/// Three different channels and none of them the specification's default of
/// black, so a row that lost the factor — or swapped two of its channels —
/// reads differently from one that kept it.
pub(crate) const EMISSIVE_FACTOR: [f32; 3] = [0.5, 0.25, 0.125];

/// [`textured_glb`]'s document with one image named by
/// **`metallicRoughnessTexture` and `occlusionTexture` together** — glTF's
/// own packing convention — and no `baseColorTexture`.
///
/// The base-colour slot is left empty for [`normal_mapped_glb`]'s reason:
/// the pages are indexed separately, so a document that filled both would
/// let a row pointed at the wrong page's layer still read the right number.
pub(crate) fn packed_glb() -> Vec<u8> {
    let (json, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
    let json = replacing(
        &json,
        r#""baseColorFactor": [0.25, 0.5, 0.75, 1.0],
      "baseColorTexture": { "index": 0, "texCoord": 0 }
    }"#,
        r#""baseColorFactor": [0.25, 0.5, 0.75, 1.0],
      "metallicRoughnessTexture": { "index": 0, "texCoord": 0 }
    },
    "occlusionTexture": { "index": 0, "texCoord": 0 }"#,
    );
    glb(&json, Some(&bin))
}

/// [`textured_glb`]'s document with its one image named by
/// **`emissiveTexture`** under [`EMISSIVE_FACTOR`], and no
/// `baseColorTexture` — [`packed_glb`]'s shape for the fourth page.
pub(crate) fn emissive_textured_glb() -> Vec<u8> {
    let (json, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
    let json = replacing(
        &json,
        r#""baseColorFactor": [0.25, 0.5, 0.75, 1.0],
      "baseColorTexture": { "index": 0, "texCoord": 0 }
    }"#,
        &format!(
            r#""baseColorFactor": [0.25, 0.5, 0.75, 1.0]
    }},
    "emissiveFactor": [{}, {}, {}],
    "emissiveTexture": {{ "index": 0, "texCoord": 0 }}"#,
            EMISSIVE_FACTOR[0], EMISSIVE_FACTOR[1], EMISSIVE_FACTOR[2]
        ),
    );
    glb(&json, Some(&bin))
}

/// **The three slots rung 3 added arrive on the seam the other two ride**,
/// each naming the image the document named and each leaving the row's page
/// column at `NO_PAGE` for the page builder.
///
/// One image from two slots is the case that matters: glTF's convention is
/// that `occlusionTexture` and `metallicRoughnessTexture` name the *same*
/// packed image, and an importer that reported one of them would build a
/// page with the occlusion channel missing and nothing saying so.
///
/// # Sabotage
///
/// `build`'s `occlusion_textures` reading `material.emissive_texture()`
/// instead of `material.occlusion_texture()`. Red on 2026-09-06 with
/// `"assertion failed: scene.occlusion_textures()[0].is_some()"`.
#[test]
fn a_material_naming_a_packed_map_reports_it_from_both_slots_and_points_no_page() {
    let scene = import_glb_bytes(&packed_glb()).expect("a packed document imports");

    let packed = scene.metallic_roughness_textures()[0]
        .expect("the material names a metallicRoughnessTexture");
    assert!(
        scene.occlusion_textures()[0].is_some(),
        "the occlusionTexture names the same image and has to be reported too"
    );
    let occlusion = scene.occlusion_textures()[0].expect("the material names an occlusionTexture");
    assert_eq!(
        (packed.image(), occlusion.image()),
        (0, 0),
        "both slots name the document's one image"
    );
    assert_eq!((packed.tex_coord(), occlusion.tex_coord()), (0, 0));
    assert!(
        scene.base_color_textures()[0].is_none() && scene.emissive_textures()[0].is_none(),
        "this document names neither of the other two slots"
    );
    assert_eq!(
        scene.materials()[0].metallic_roughness_occlusion_texture,
        GpuMaterial::NO_PAGE,
        "the column is a page layer, which only the page builder knows"
    );
}

/// **`emissiveTexture` arrives beside the factor, not instead of it.**
///
/// The two are one product — topic 43 §2's rung 3
/// — and they split across the seam: the factor is a number and rides the
/// row, the image is a page layer and rides beside it.
///
/// # Sabotage
///
/// `build`'s `emissive_textures` collecting `None` for every material.
/// Red on 2026-09-06 with `"the material names an emissiveTexture"`.
#[test]
fn a_material_with_an_emissive_texture_reports_its_image_and_keeps_the_factor() {
    let scene = import_glb_bytes(&emissive_textured_glb()).expect("an emissive document imports");

    let texture = scene.emissive_textures()[0].expect("the material names an emissiveTexture");
    assert_eq!(texture.image(), 0);
    assert_eq!(texture.tex_coord(), 0);
    assert_eq!(
        scene.materials()[0].emissive,
        EMISSIVE_FACTOR,
        "the factor is a number and stays on the row"
    );
    assert_eq!(
        scene.materials()[0].emissive_texture,
        GpuMaterial::NO_PAGE,
        "the layer beside it is the page builder's"
    );
}

#[test]
fn a_material_naming_none_of_the_three_new_slots_reports_none_for_each() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(scene.metallic_roughness_textures(), [None]);
    assert_eq!(scene.occlusion_textures(), [None]);
    assert_eq!(scene.emissive_textures(), [None]);
}

#[test]
fn a_primitive_with_a_tangent_accessor_reads_it_back_with_its_handedness() {
    let scene = import_glb_bytes(&tangent_glb(TANGENTS.len())).expect("a TANGENT document imports");

    let primitive = &scene.meshes()[0].primitives()[0];
    assert_eq!(
        primitive.tangents(),
        TANGENTS,
        "the float4 arrives as authored, the handedness in `w` included"
    );
    assert!(
        primitive.tangents().iter().any(|tangent| tangent[3] < 0.0),
        "the fixture must carry a left-handed vertex or the sign is untested",
    );
}

#[test]
fn a_primitive_with_no_tangent_accessor_reads_back_no_tangents() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert!(
        scene.meshes()[0].primitives()[0].tangents().is_empty(),
        "an absent attribute is empty, not invented",
    );
}

/// A `TANGENT` shorter than `POSITION` is refused the way every other
/// short attribute is: there is no vertex for the missing entries to
/// belong to, and filling them would be this crate inventing a frame.
#[test]
fn a_tangent_shorter_than_position_is_refused() {
    let error = import_glb_bytes(&tangent_glb(TANGENTS.len() - 1))
        .expect_err("a short TANGENT makes the document malformed");

    assert!(
        error
            .to_string()
            .contains("has 3 positions and 2 TANGENT values"),
        "the refusal does not name the attribute and both counts: {error}",
    );
}

#[test]
fn a_material_with_a_normal_texture_reports_its_image_and_keeps_the_scale_on_the_row() {
    let scene = import_glb_bytes(&normal_mapped_glb()).expect("the fixture imports");

    let texture = scene.normal_textures()[0].expect("the material names a normalTexture");
    assert_eq!(texture.image(), 0, "the document's only image");
    assert_eq!(texture.tex_coord(), 0);
    assert!(
        scene.base_color_textures()[0].is_none(),
        "the fixture moved its one texture to the normal slot",
    );

    assert_eq!(
        scene.materials()[0].normal_scale,
        NORMAL_SCALE,
        "the scale is a material factor, so it rides on the row",
    );
    assert_eq!(
        scene.materials()[0].normal_texture,
        GpuMaterial::NO_PAGE,
        "the index is a page layer, which this module cannot know",
    );
}

#[test]
fn a_material_with_no_normal_texture_reports_none_and_the_specification_default_scale() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert!(scene.normal_textures()[0].is_none());
    assert_eq!(
        scene.materials()[0].normal_scale,
        GLTF_DEFAULT_MATERIAL.normal_scale,
        "glTF's own `normalTexture.scale` default, which is a map left as authored",
    );
}

/// The fixture with `fields` spliced into its one material, which is how
/// each alpha-mode test writes the `alphaMode` and `alphaCutoff` the
/// document under test declares.
fn material_with(fields: &str) -> String {
    replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#""name": "paint","#,
        &format!(r#""name": "paint", {fields},"#),
    )
}

/// **A `MASK` material arrives as a cutout at the threshold the document
/// wrote.** The mode is the bit the shaders test and the cutoff is what
/// they test against, so a row carrying one without the other discards at
/// the wrong alpha or not at all.
#[test]
fn a_mask_material_carries_the_mask_bit_and_its_authored_cutoff() {
    let json = material_with(r#""alphaMode": "MASK", "alphaCutoff": 0.25"#);
    let scene = import_glb(&json).expect("the fixture imports");

    assert_eq!(
        scene.materials()[0].flags,
        GpuMaterial::ALPHA_MODE_MASK,
        "`alphaMode: MASK` did not set the bit the shaders test, so a cutout surface draws \
             solid and casts a solid shadow",
    );
    assert_eq!(
        scene.materials()[0].alpha_cutoff,
        0.25,
        "the document's own `alphaCutoff` was not carried, so the cutout is at somebody \
             else's alpha",
    );

    let warnings = import_warnings(&json);
    assert!(
        warnings.is_empty(),
        "`MASK` is imported rather than dropped, so nothing about it is worth a line: \
             {warnings:#?}",
    );
}

/// A `MASK` material with no `alphaCutoff` cuts where the specification
/// says. The comparand is [`GLTF_DEFAULT_MATERIAL`] rather than
/// `UNTINTED`'s row for that constant's own reason: this asserts what the
/// *document* means, which a change to the engine's neutral row must not
/// be able to move.
#[test]
fn a_mask_material_with_no_cutoff_takes_the_specification_default() {
    let scene = import_glb(&material_with(r#""alphaMode": "MASK""#)).expect("it imports");

    assert_eq!(
        scene.materials()[0].flags,
        GpuMaterial::ALPHA_MODE_MASK,
        "the bit is the mode's alone — an absent `alphaCutoff` does not unmask a material",
    );
    assert_eq!(
        scene.materials()[0].alpha_cutoff,
        GLTF_DEFAULT_MATERIAL.alpha_cutoff,
        "a `MASK` material that wrote no threshold must cut at glTF's own default, not at \
             whatever the row was initialised to",
    );
}

/// An explicit `alphaMode: "OPAQUE"` is the absence of every bit, which is
/// also what the unmarked fixture means — the two have to agree, or `flags`
/// says two different things about one material the document describes two
/// ways.
#[test]
fn an_opaque_material_carries_no_bits() {
    let scene = import_glb(&material_with(r#""alphaMode": "OPAQUE""#)).expect("it imports");

    assert_eq!(
        scene.materials()[0].flags,
        0,
        "`OPAQUE` set a bit, and every bit `flags` carries selects a behaviour this \
             material asked not to have",
    );
    assert_eq!(
        scene.materials()[0].alpha_cutoff,
        GLTF_DEFAULT_MATERIAL.alpha_cutoff,
        "an unmasked row's cutoff is never compared against, so it reports the default",
    );
}

/// **`doubleSided: true` sets the bit the renderer routes and the shader
/// reverses a normal through; absent and `false` do not.**
///
/// Three documents rather than one, because the failure this catches is a
/// reader that answers the same thing whatever the document says — an
/// importer that set the bit unconditionally would satisfy the first
/// assertion alone, and one that never set it would satisfy the other two.
/// The absent case is separate from the explicit `false` because glTF's
/// default for the key is what the absent case has to mean, and nothing
/// else in this module would notice if it stopped.
///
/// The alpha mode is asserted beside it in the third case: the two are
/// independent modes and a material can carry both, which is what a cutout
/// leaf card is.
#[test]
fn a_double_sided_material_carries_the_bit_and_a_single_sided_one_does_not() {
    let json = material_with(r#""doubleSided": true"#);
    let scene = import_glb(&json).expect("the fixture imports");
    assert_eq!(
        scene.materials()[0].flags,
        GpuMaterial::DOUBLE_SIDED,
        "`doubleSided: true` did not set the bit the renderer routes on, so the surface is \
             back-face culled and a leaf card is invisible from behind",
    );

    let warnings = import_warnings(&json);
    assert!(
        warnings.is_empty(),
        "`doubleSided` is imported rather than dropped, so nothing about it is worth a \
             line: {warnings:#?}",
    );

    for (document, what) in [
        (
            material_with(r#""doubleSided": false"#),
            "an explicit `false`",
        ),
        (
            triangle_json(BIN_CHUNK_BUFFER),
            "a material with no such key",
        ),
    ] {
        let scene = import_glb(&document).expect("the fixture imports");
        assert_eq!(
            scene.materials()[0].flags & GpuMaterial::DOUBLE_SIDED,
            0,
            "{what} must leave the material single-sided, which is glTF's own default",
        );
    }

    let both = import_glb(&material_with(
        r#""doubleSided": true, "alphaMode": "MASK""#,
    ))
    .expect("the fixture imports");
    assert_eq!(
        both.materials()[0].flags,
        GpuMaterial::ALPHA_MODE_MASK | GpuMaterial::DOUBLE_SIDED,
        "a cutout leaf card carries both modes, and an importer that let one overwrite the \
             other would draw a solid card or a single-sided hole",
    );
}

/// **`BLEND` is recorded as `OPAQUE`, and that is the decision rather than
/// an oversight.** This renderer builds no blended pipeline at all —
/// topic 43 §3 — so no bit could honour it, and
/// reading it as `MASK` would punch a hard-edged hole through a surface the
/// author asked to fade. The warning is what keeps the loss visible, so it
/// is asserted beside the row.
#[test]
fn a_blend_material_is_recorded_opaque_and_named_in_a_warning() {
    let json = material_with(r#""alphaMode": "BLEND""#);
    let scene = import_glb(&json).expect("the fixture imports");

    assert_eq!(
        scene.materials()[0].flags,
        0,
        "`BLEND` must flatten onto `OPAQUE`: a mask bit here cuts holes through a surface \
             the document asked to fade smoothly",
    );
    assert_eq!(
        scene.materials()[0].alpha_cutoff,
        GLTF_DEFAULT_MATERIAL.alpha_cutoff,
        "a flattened `BLEND` has no threshold of its own to report",
    );

    let warnings = import_warnings(&json);
    let named = warnings
        .iter()
        .find(|line| line.contains("BLEND"))
        .unwrap_or_else(|| panic!("no line named the flattened material: {warnings:#?}"));
    assert!(
        named.contains("1 BLEND material(s)"),
        "the line does not name the count, so a document with fifty reads like one: {named}",
    );
}

#[test]
fn a_minimal_glb_yields_its_positions_normals_texcoords_indices_and_material() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(scene.meshes().len(), 1);
    let mesh = &scene.meshes()[0];
    assert_eq!(mesh.name(), Some("triangle"));
    assert_eq!(mesh.primitives().len(), 1);

    let primitive = &mesh.primitives()[0];
    assert_eq!(primitive.positions(), POSITIONS);
    assert_eq!(primitive.normals(), NORMALS);
    assert_eq!(primitive.tex_coords(), TEX_COORDS);
    assert_eq!(
        primitive.indices(),
        INDICES.map(u32::from),
        "u16 indices arrive widened, not reinterpreted"
    );
    assert_eq!(primitive.material(), Some(0));

    // The fixture names a `baseColorFactor` and neither shading factor, so
    // the colour is the document's and the other two are the
    // specification's defaults — which is the whole of what the mapping
    // claims.
    assert_eq!(
        scene.materials(),
        [GpuMaterial {
            base_color: BASE_COLOR,
            ..GLTF_DEFAULT_MATERIAL
        }]
    );
}

/// The rigged fixture's skin arrives whole: its joints, its skeleton root
/// and its bind matrices.
///
/// `gltf_check`'s `the_rigged_fixture_holds_the_rig_it_declares` reads the
/// same numbers straight out of the `gltf` crate. This one asserts they
/// made it through **[`import_gltf`]**, which is a different claim: the
/// fixture was already valid before this importer read a skin at all.
#[test]
fn a_skin_arrives_with_its_joints_skeleton_and_bind_matrices() {
    let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(scene.skins().len(), 1, "{:?}", scene.skins());
    let skin = &scene.skins()[0];
    assert_eq!(skin.name(), Some("rig"));
    assert_eq!(
        skin.joints(),
        [1, 2],
        "the joints are the two joint nodes, in the document's order"
    );
    assert_eq!(
        skin.skeleton(),
        Some(1),
        "the fixture names node 1 as the skeleton root"
    );
    assert_eq!(
        skin.inverse_binds(),
        INVERSE_BIND.map(|matrix| Mat4::from_cols_array(&matrix)),
        "the bind matrices are the constants, column-major, in joint order"
    );
}

/// **Only the node that wears the skin says so**, and that is the fact a
/// consumer needs to tell a skinned instance from scenery.
///
/// A mesh's `JOINTS_0` cannot decide it. The same rigged mesh may be drawn
/// again under a node with no skin — legal glTF, and what
/// `apps/viewer`'s demo document does deliberately — and skinning that
/// second copy would draw it wherever the joints happen to be. So the
/// skinned node reporting `Some` is asserted together with the joint nodes
/// reporting `None`: the first alone would pass for an importer that put a
/// skin on every node.
#[test]
fn only_the_node_that_wears_the_skin_reports_one() {
    let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER)).unwrap();

    let worn: Vec<Option<usize>> = scene.nodes().iter().map(GltfNode::skin).collect();
    assert_eq!(
        worn,
        [Some(0), None, None],
        "the fixture's node 0 draws the mesh and wears skin 0; nodes 1 and 2 are its joints"
    );
}

/// The rigged fixture's one clip arrives with its channel's target, its
/// keyframe times and its rotations.
#[test]
fn a_clip_arrives_with_its_channel_times_and_rotations() {
    let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(scene.clips().len(), 1, "{:?}", scene.clips());
    let clip = &scene.clips()[0];
    assert_eq!(clip.name(), Some("wave"));
    assert_eq!(clip.channels().len(), 1);

    let channel = &clip.channels()[0];
    assert_eq!(
        channel.node(),
        2,
        "the channel turns the tip joint, which is node 2"
    );
    assert_eq!(channel.times(), CLIP_TIMES);
    assert_eq!(channel.interpolation(), GltfInterpolation::Linear);
    assert_eq!(
        channel.samples(),
        &GltfSamples::Rotations(CLIP_ROTATIONS.to_vec()),
        "a rotation channel's samples are quaternions, and the variant says so"
    );
}

/// The rigged fixture's primitive carries its per-vertex binding, one
/// entry per position.
///
/// The lengths are asserted against `positions` rather than against `3`:
/// what the consumer needs is that a vertex index reaches all four arrays,
/// and a hard-coded count would still pass if the geometry moved and the
/// binding did not.
#[test]
fn a_skinned_primitive_carries_its_joints_and_weights() {
    let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER)).unwrap();

    let primitive = &scene.meshes()[0].primitives()[0];
    assert_eq!(
        primitive.joints(),
        JOINTS,
        "JOINTS_0 is UNSIGNED_SHORT here and arrives widened, not reinterpreted"
    );
    assert_eq!(primitive.weights(), WEIGHTS);
    assert_eq!(primitive.joints().len(), primitive.positions().len());
    assert_eq!(primitive.weights().len(), primitive.positions().len());
}

/// A skin with more joints than bind matrices is refused.
///
/// [`GltfSkin::inverse_binds`] promises one matrix per joint, and the only
/// thing that makes that true is this refusal: the accessor stays in range
/// at a lower count, so nothing in [`crate::gltf_check`] has any reason to
/// object to a document whose two arrays are different lengths.
#[test]
fn a_skin_with_fewer_bind_matrices_than_joints_is_refused() {
    let json = replacing(
        &rigged_json(BIN_CHUNK_BUFFER),
        r#"{ "bufferView": 6, "componentType": 5126, "count": 2, "type": "MAT4" }"#,
        r#"{ "bufferView": 6, "componentType": 5126, "count": 1, "type": "MAT4" }"#,
    );
    let error = import_rigged_glb(&json).unwrap_err().to_string();
    assert!(
        error.contains("skin 0 has 2 joints and 1 inverse bind matrices"),
        "unexpected reason: {error}"
    );
}

/// A channel whose samples do not line up with its keyframes is refused.
///
/// The mutation is the fixture's `interpolation`, not its data: under
/// `CUBICSPLINE` a sampler stores an in-tangent, a value and an out-tangent
/// per keyframe, so the two rotations that satisfy `LINEAR` are a third of
/// what this asks for. Nothing before `check_sample_count` objects — the
/// accessors are all in range and the right types — so this is the only
/// thing standing between a player and a channel it would index off the
/// end of.
#[test]
fn a_channel_with_the_wrong_number_of_samples_is_refused() {
    let json = replacing(
        &rigged_json(BIN_CHUNK_BUFFER),
        r#""interpolation": "LINEAR""#,
        r#""interpolation": "CUBICSPLINE""#,
    );
    let error = import_rigged_glb(&json).unwrap_err().to_string();
    assert!(
        error.contains("has 2 keyframes and 2 rotation values")
            && error.contains("CubicSpline interpolation wants 6"),
        "unexpected reason: {error}"
    );
}

/// A document with no rig says so by being empty, not by being absent.
///
/// The point of the four assertions is that presence stays
/// **distinguishable** from absence: a reader that filled these arrays with
/// defaults — an identity bind matrix per joint, a full weight on joint
/// zero — would satisfy every test above and make an unrigged mesh
/// indistinguishable from a rigged one.
#[test]
fn a_document_with_no_skin_or_animation_has_neither() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert!(
        scene.skins().is_empty(),
        "the triangle has no skin: {:?}",
        scene.skins()
    );
    assert!(
        scene.clips().is_empty(),
        "the triangle has no animation: {:?}",
        scene.clips()
    );
    let primitive = &scene.meshes()[0].primitives()[0];
    assert!(
        primitive.joints().is_empty(),
        "and so no JOINTS_0: {:?}",
        primitive.joints()
    );
    assert!(
        primitive.weights().is_empty(),
        "and so no WEIGHTS_0: {:?}",
        primitive.weights()
    );
}

/// The same document with its bytes in a file instead of a chunk. Asserted
/// against the `.glb` rather than against a second copy of the expected
/// values, so the two paths cannot drift.
/// The sibling key is built in URI space on every platform.
///
/// This is the test that would have caught the Windows failure on Linux.
/// Asserting the *imported scene* could not: `Path::join` produces the right
/// string here and the wrong one on Windows, so the round trip passes on the
/// machine the code was written on and fails on the runner. Asserting the
/// join itself, over strings, is the same claim on both.
#[test]
fn a_buffer_beside_a_gltf_is_named_with_a_slash_whatever_the_platform() {
    assert_eq!(uri_parent(Path::new("meshes/scene.gltf")), "meshes");
    assert_eq!(uri_parent(Path::new("a/b/scene.gltf")), "a/b");
    assert_eq!(uri_parent(Path::new("scene.gltf")), "");

    assert_eq!(uri_sibling("meshes", "triangle.bin"), "meshes/triangle.bin");
    assert_eq!(uri_sibling("a/b", "c.bin"), "a/b/c.bin");
    // A gltf at the root names its sibling with no prefix at all — not
    // "/c.bin", which `canonical_key` refuses as an absolute path.
    assert_eq!(uri_sibling("", "c.bin"), "c.bin");

    // The property the failure was about: nothing this builds contains a
    // separator `canonical_key` will not accept.
    for key in [
        uri_sibling(uri_parent(Path::new("meshes/scene.gltf")), "triangle.bin"),
        uri_sibling(uri_parent(Path::new("scene.gltf")), "triangle.bin"),
    ] {
        assert!(!key.contains('\\'), "{key} carries a platform separator");
    }
}

#[test]
fn a_gltf_reads_its_buffer_from_the_bin_file_beside_it() {
    let from_file = import_gltf_text(&triangle_json(EXTERNAL_BUFFER), &triangle_bin()).unwrap();
    assert_eq!(
        from_file,
        import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap()
    );
}

/// The node table is the document's array, so it holds the parent that
/// draws nothing as well as the child that draws the mesh — and an instance
/// says which entry it came from.
#[test]
fn every_node_is_in_the_table_with_its_name_and_what_it_draws() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(
        scene.nodes(),
        [
            GltfNode {
                name: Some("root".to_owned()),
                mesh: None,
                skin: None,
                local_transform: Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0)),
                children: vec![1],
                lod_nodes: Vec::new(),
            },
            GltfNode {
                name: Some("leaf".to_owned()),
                mesh: Some(0),
                skin: None,
                local_transform: Mat4::from_translation(glam::Vec3::new(0.0, 5.0, 0.0)),
                children: Vec::new(),
                lod_nodes: Vec::new(),
            },
        ]
    );
    assert_eq!(
        scene.instances()[0].node(),
        1,
        "the drawing node, not the root above it"
    );
}

/// A node's transform is its own, and composing the chain by hand
/// reproduces the instance the walk emitted.
///
/// The `assert_ne!` is what makes the first claim testable at all: the
/// fixture's leaf sits under a translated parent, so a
/// `local_transform` that had quietly been the composed one would pass
/// every equality below and fail this.
#[test]
fn a_nodes_local_transform_is_its_own_and_composes_into_its_instance() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    let root = scene.nodes()[0].local_transform();
    let leaf = scene.nodes()[1].local_transform();
    assert_eq!(
        leaf,
        Mat4::from_translation(glam::Vec3::new(0.0, 5.0, 0.0)),
        "the leaf's own (0, 5, 0), with nothing of the root in it"
    );

    let instance = scene.instances()[0];
    assert_eq!(instance.node(), 1);
    assert_ne!(
        leaf.to_cols_array(),
        instance.transform(),
        "a local transform that equalled the composed one here would be the composed one"
    );
    assert_eq!(
        (root * leaf).to_cols_array(),
        instance.transform(),
        "parent then child is what the walk composed"
    );
}

/// **The walk composes parent then child, and this is the fixture that can
/// tell.**
///
/// `a_nodes_local_transform_is_its_own_and_composes_into_its_instance`
/// asserts the same product, and cannot catch the order: both transforms in
/// that document are translations, and translations commute — reverse the
/// multiplication in `flatten` and every assertion there still passes.
/// Turning the parent is what makes the two orders different matrices, and
/// the order is the arithmetic a joint palette is about to depend on.
///
/// A quarter turn about `Z` over a child five metres up its own `Y`: parent
/// then child swings the leaf onto `-X`, child then parent leaves it on
/// `+Y` and moves the parent's own offset instead.
#[test]
fn the_walk_composes_the_parent_before_the_child() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#"{ "name": "root", "translation": [10.0, 0.0, 0.0], "children": [1] }"#,
        r#"{ "name": "root", "rotation": [0.0, 0.0, 0.70710678, 0.70710678], "children": [1] }"#,
    );
    let scene = import_glb(&json).unwrap();

    let root = scene.nodes()[0].local_transform();
    let leaf = scene.nodes()[1].local_transform();
    let composed = Mat4::from_cols_array(&scene.instances()[0].transform());

    // Where the leaf's own origin ends up, which is the whole of what the
    // order decides: a quarter turn about `Z` carries `+Y` to `-X`.
    let placed = composed.transform_point3(glam::Vec3::ZERO);
    assert!(
        placed.distance(glam::Vec3::new(-5.0, 0.0, 0.0)) < 1e-5,
        "the parent's turn has to reach the leaf: it sits at {placed:?}",
    );
    assert_eq!(composed, root * leaf, "parent then child");
    assert_ne!(
        root * leaf,
        leaf * root,
        "a fixture where the two orders agree would prove nothing",
    );
}

/// A node the scene graph never names still carries its transform and its
/// children: those are the document's own fields, not something the walk
/// discovered.
///
/// The scene here names the leaf alone, so the root above it is reachable
/// from no scene — which glTF permits, and which is the case a `parent`
/// field could not have answered.
#[test]
fn a_node_no_scene_reaches_still_carries_its_transform_and_children() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#""scenes": [{ "nodes": [0] }]"#,
        r#""scenes": [{ "nodes": [1] }]"#,
    );
    let scene = import_glb(&json).unwrap();

    assert_eq!(
        scene.nodes()[0].children(),
        [1],
        "the unreached root still says what hangs off it"
    );
    assert_eq!(
        scene.nodes()[0].local_transform(),
        Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0)),
    );
    assert_eq!(
        scene.instances()[0].transform(),
        Mat4::from_translation(glam::Vec3::new(0.0, 5.0, 0.0)).to_cols_array(),
        "and none of it reached the instance, because the walk started below it"
    );
}

/// The rigged fixture's two joints arrive as a hierarchy: the tip hangs off
/// the root, and carries the translation the root's bind matrix inverts.
///
/// Neither joint draws a mesh, so none of this is in
/// [`GltfScene::instances`] — the node table is the only place a skeleton
/// can be read from.
#[test]
fn a_joint_node_carries_its_own_transform_and_its_children() {
    let scene = import_rigged_glb(&rigged_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(
        scene.nodes()[1].children(),
        [2],
        "the tip joint hangs off the root joint"
    );
    assert!(scene.nodes()[0].children().is_empty(), "the skinned mesh");
    assert!(scene.nodes()[2].children().is_empty(), "the tip is a leaf");

    let tip = Mat4::from_translation(glam::Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(
        scene.nodes()[2].local_transform(),
        tip,
        "the tip's own translation, in the document's units"
    );
    assert_eq!(
        Mat4::from_cols_array(&INVERSE_BIND[1]) * tip,
        Mat4::IDENTITY,
        "the fixture's bind matrix is the inverse of where the tip stands"
    );

    assert_eq!(
        scene.nodes()[1].local_transform(),
        Mat4::IDENTITY,
        "a node the document gives no transform is at the identity"
    );
    assert!(
        scene
            .instances()
            .iter()
            .all(|instance| instance.node() != 2),
        "a joint draws nothing, so it is in no instance"
    );
}

/// `MSFT_lod` has no feature of its own in `gltf`, so this is the raw
/// extension map being read — and the ids arriving as node indices is what
/// `crate::lod_resolve` rests on.
#[test]
fn a_nodes_msft_lod_ids_are_read_from_the_raw_extension() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        r#"{ "name": "leaf", "mesh": 0, "translation": [0.0, 5.0, 0.0] }"#,
        r#"{ "name": "leaf", "mesh": 0, "extensions": { "MSFT_lod": { "ids": [0] } } }"#,
    );
    let scene = import_glb(&json).unwrap();

    assert!(
        scene.nodes()[0].lod_nodes().is_empty(),
        "no extension, no ids"
    );
    assert_eq!(scene.nodes()[1].lod_nodes(), [0]);
}

/// The fixture's mesh hangs off a child node, so the instance transform is
/// only right if the parent's translation was composed into it.
#[test]
fn a_nodes_transform_composes_with_every_parent_above_it() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();

    assert_eq!(scene.instances().len(), 1, "one node draws a mesh");
    let instance = scene.instances()[0];
    assert_eq!(instance.mesh(), 0);
    assert_eq!(
        instance.transform(),
        Mat4::from_translation(glam::Vec3::new(10.0, 5.0, 0.0)).to_cols_array(),
        "the leaf's (0, 5, 0) under the root's (10, 0, 0)"
    );
}

#[test]
fn a_primitive_with_no_index_accessor_is_given_the_trivial_indices() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        "\n      \"indices\": 3,",
        "",
    );
    let scene = import_glb(&json).unwrap();
    let primitive = &scene.meshes()[0].primitives()[0];
    assert_eq!(primitive.indices(), [0, 1, 2]);
    assert_eq!(primitive.positions().len(), 3);
}

/// A material with no `pbrMetallicRoughness` block at all imports as the
/// specification's defaults, and **that is no longer the engine's neutral
/// row**.
///
/// The assertion moved rather than the importer: glTF's default material is
/// a fully rough conductor, and a mapping that quietly substituted
/// [`GpuMaterial::UNTINTED`] for it would be reporting the engine's
/// preference as the document's content. `GLTF_DEFAULT_MATERIAL` is what it
/// is asserted against, and the `assert_ne!` below is what says the two are
/// genuinely different rows — without it this test would pass again the day
/// somebody made them equal.
#[test]
fn a_material_with_no_factors_takes_the_gltf_defaults_and_a_primitive_may_name_none() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        "\"pbrMetallicRoughness\": { \"baseColorFactor\": [0.25, 0.5, 0.75, 1.0] }",
        // Something inert in the block's place, and `false` rather than
        // `true` because the importer reads this key now: glTF's own
        // default for it, so the row it produces is still the default
        // material this test is about.
        "\"doubleSided\": false",
    );
    let json = replacing(&json, ",\n      \"material\": 0", "");
    let scene = import_glb(&json).unwrap();

    assert_eq!(scene.materials(), [GLTF_DEFAULT_MATERIAL]);
    assert_ne!(
        GLTF_DEFAULT_MATERIAL,
        GpuMaterial::UNTINTED,
        "an imported default material used to be the untinted row and is not one any more; \
             a tree in which they are equal again is one where this test says nothing"
    );
    assert_eq!(scene.meshes()[0].primitives()[0].material(), None);
}

/// **Emission imports as one radiance, not as a factor and a multiplier.**
///
/// glTF splits it: `emissiveFactor` is capped at one per channel, and
/// `KHR_materials_emissive_strength` is what lifts a surface above white.
/// `GpuMaterial::emissive` is their product, so an importer that read only
/// the factor would clamp every bright emitter to at most one and produce a
/// document that loads, renders and is dimmer than it says — which is why
/// the second half of this asserts a value above one specifically.
#[test]
fn an_emissive_material_imports_the_factor_times_the_strength() {
    const FACTOR: &str = "\"emissiveFactor\": [0.5, 0.25, 0.125]";
    let with_factor = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        "\"pbrMetallicRoughness\"",
        &format!("{FACTOR}, \"pbrMetallicRoughness\""),
    );
    assert_eq!(
        import_glb(&with_factor).unwrap().materials()[0].emissive,
        [0.5, 0.25, 0.125],
        "a document with no strength extension multiplies by one, which is what the \
             extension's own absence means",
    );

    let with_strength = replacing(
        &with_factor,
        FACTOR,
        &format!(
            "{FACTOR}, \"extensions\": {{ \"KHR_materials_emissive_strength\": \
                 {{ \"emissiveStrength\": 8.0 }} }}"
        ),
    );
    assert_eq!(
        import_glb(&with_strength).unwrap().materials()[0].emissive,
        [4.0, 2.0, 1.0],
        "the strength must multiply the factor, and the result must be allowed above one",
    );
}

/// Stage 6's import rule (`docs/notes/tooling.md`): unsupported features
/// log and skip rather than failing the load.
#[test]
fn a_primitive_that_is_not_a_triangle_list_is_skipped_and_the_rest_still_loads() {
    let json = replacing(
        &triangle_json(BIN_CHUNK_BUFFER),
        "\"indices\": 3,",
        "\"indices\": 3, \"mode\": 0,",
    );
    let scene = import_glb(&json).unwrap();

    assert_eq!(scene.meshes().len(), 1, "the mesh keeps its entry");
    assert!(scene.meshes()[0].primitives().is_empty());
    assert_eq!(
        scene.instances().len(),
        1,
        "and the node naming it still resolves"
    );
}

#[test]
fn a_document_with_no_scene_still_yields_its_meshes_and_no_instances() {
    let json = replacing(&triangle_json(BIN_CHUNK_BUFFER), "\n  \"scene\": 0,", "");
    let json = replacing(&json, "\n  \"scenes\": [{ \"nodes\": [0] }],", "");
    let scene = import_glb(&json).unwrap();

    assert!(scene.instances().is_empty());
    assert_eq!(scene.meshes()[0].primitives()[0].positions(), POSITIONS);
}

/// A document with scenes but no `scene` renders the first one, which is
/// what every other loader does with a file the spec leaves undefined.
#[test]
fn a_document_with_no_default_scene_falls_back_to_the_first_one() {
    let json = replacing(&triangle_json(BIN_CHUNK_BUFFER), "\n  \"scene\": 0,", "");
    let scene = import_glb(&json).unwrap();
    assert_eq!(scene.instances().len(), 1);
}

/// The whole reason `AssetSource::read` may not block: a source that does
/// not have the bytes yet makes the import a state rather than a failure,
/// for the document and for a buffer it names alike.
#[test]
fn an_import_is_pending_while_either_the_document_or_a_buffer_is_not_resident() {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Debug, Default)]
    struct Pending {
        files: HashMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl AssetSource for Pending {
        fn read(&self, key: &Path) -> Result<Vec<u8>, StorageError> {
            let name = key.to_string_lossy().into_owned();
            self.asked.borrow_mut().push(name.clone());
            self.files
                .get(&name)
                .cloned()
                .ok_or_else(|| StorageError::Pending(PathBuf::from(name)))
        }
    }

    let mut source = Pending::default();
    assert!(
        matches!(
            import_gltf(&source, Path::new("model.gltf")),
            Err(StorageError::Pending(_))
        ),
        "the document itself has not arrived"
    );

    source.files.insert(
        "model.gltf".to_string(),
        triangle_json(EXTERNAL_BUFFER).into_bytes(),
    );
    assert!(
        matches!(
            import_gltf(&source, Path::new("model.gltf")),
            Err(StorageError::Pending(_))
        ),
        "the document is here and its buffer is not"
    );
    assert_eq!(
        source.asked.borrow().last().map(String::as_str),
        Some("triangle.bin"),
        "the buffer uri resolved beside the document"
    );

    source
        .files
        .insert("triangle.bin".to_string(), triangle_bin());
    let scene = import_gltf(&source, Path::new("model.gltf")).unwrap();
    assert_eq!(scene.meshes()[0].primitives()[0].positions(), POSITIONS);
}

#[test]
fn a_documents_images_arrive_encoded_beside_the_material_that_names_them() {
    let png = png_bytes(2, 2, &IMAGE_TEXELS);
    let scene = import_glb_bytes(&textured_glb(&png, "image/png", 0)).unwrap();

    assert_eq!(scene.images().len(), 1);
    let image = &scene.images()[0];
    assert_eq!(image.name(), Some("paint"));
    assert_eq!(image.mime(), Some("image/png"));
    assert_eq!(
        image.bytes(),
        Ok(&png[..]),
        "the bytes are the file's own, byte for byte and still encoded"
    );

    assert_eq!(scene.base_color_textures().len(), scene.materials().len());
    let texture = scene.base_color_textures()[0].expect("the material names a texture");
    assert_eq!(texture.image(), 0);
    assert_eq!(texture.tex_coord(), 0);
    assert_eq!(
        scene.materials()[0].base_color_texture,
        GpuMaterial::UNTINTED.base_color_texture,
        "the row's texture column stays at UNTINTED's own: a page layer is not \
             something this module can know"
    );
}

#[test]
fn a_material_naming_no_texture_has_no_entry_beside_it() {
    let scene = import_glb(&triangle_json(BIN_CHUNK_BUFFER)).unwrap();
    assert_eq!(scene.images(), []);
    assert_eq!(scene.base_color_textures(), [None]);
}

#[test]
fn the_texcoord_a_material_asks_for_is_reported_rather_than_assumed() {
    let png = png_bytes(2, 2, &IMAGE_TEXELS);
    let scene = import_glb_bytes(&textured_glb(&png, "image/png", 1)).unwrap();
    assert_eq!(
        scene.base_color_textures()[0]
            .expect("a texture")
            .tex_coord(),
        1,
        "TEXCOORD_1 is not read, and a silent 0 here would sample the wrong UVs \
             instead of saying so"
    );
}

/// The asymmetry the module docs argue for: a buffer that will not resolve
/// fails the import, and an image that will not resolve is a skipped image.
#[test]
fn an_image_uri_the_source_cannot_read_is_skipped_and_the_document_still_imports() {
    let (json, bin) = textured_parts(b"unused", "image/png", 0);
    let json = replacing(
        &json,
        r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
        r#"{ "name": "paint", "uri": "paint.png" }"#,
    );
    let scene = import_glb_bytes(&glb(&json, Some(&bin)))
        .expect("a missing texture is not a missing model");

    let why = scene.images()[0]
        .bytes()
        .expect_err("the file is not beside the document");
    assert!(
        why.contains("paint.png") && why.contains("could not be read"),
        "the reason names the uri that failed: {why}"
    );
    assert_eq!(
        scene.meshes()[0].primitives()[0].positions(),
        POSITIONS,
        "the geometry is untouched by the texture that was not there"
    );
}

#[test]
fn a_data_uri_image_is_skipped_where_a_data_uri_buffer_is_refused() {
    let (json, bin) = textured_parts(b"unused", "image/png", 0);
    let json = replacing(
        &json,
        r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
        r#"{ "name": "paint", "uri": "data:image/png;base64,iVBORw0KGgo=" }"#,
    );
    let scene = import_glb_bytes(&glb(&json, Some(&bin))).expect("still imports");
    let why = scene.images()[0].bytes().unwrap_err();
    assert!(
        why.contains("base64"),
        "the reason says what is missing: {why}"
    );
}

#[test]
fn an_image_uri_resolves_beside_the_document_like_a_buffer_uri_does() {
    let png = png_bytes(2, 2, &IMAGE_TEXELS);
    let (json, bin) = textured_parts(b"unused", "image/png", 0);
    let json = replacing(
        &json,
        r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
        r#"{ "name": "paint", "uri": "paint.png" }"#,
    );

    let assets = Assets::new();
    assets.write("meshes/model.glb", &glb(&json, Some(&bin)));
    assets.write("meshes/paint.png", &png);
    let scene = assets.import("meshes/model.glb").unwrap();

    assert_eq!(
        scene.images()[0].bytes(),
        Ok(&png[..]),
        "`meshes/paint.png`, not `paint.png`: a uri is relative to the document's key"
    );
    assert_eq!(
        scene.images()[0].mime(),
        None,
        "a uri image may declare no mimeType, and this one does not"
    );
}

#[test]
fn an_image_uri_escaping_the_asset_root_is_skipped_rather_than_read() {
    let (json, bin) = textured_parts(b"unused", "image/png", 0);
    let json = replacing(
        &json,
        r#"{ "name": "paint", "bufferView": 4, "mimeType": "image/png" }"#,
        r#"{ "name": "paint", "uri": "../../secret.png" }"#,
    );
    let assets = Assets::new();
    assets.write("meshes/model.glb", &glb(&json, Some(&bin)));
    std::fs::write(assets.outside().join("secret.png"), b"not yours").unwrap();

    let scene = assets.import("meshes/model.glb").unwrap();
    let why = scene.images()[0].bytes().unwrap_err();
    assert!(
        why.contains("secret.png"),
        "the reason names the key that was refused: {why}"
    );
}
