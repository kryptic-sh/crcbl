//! `KHR_materials_ior` and `KHR_materials_specular`: the dielectric
//! reflectance a glTF material's specular lobe is drawn with.
//!
//! **The factors, reduced to the row's two numbers.** Every term of the
//! reflectance the extensions define is a per-material constant, so the
//! importer multiplies them out once — [`dielectric_specular`] — into
//! [`GpuMaterial::specular_f0`] and [`GpuMaterial::specular_f90`], and
//! `shaders/mesh.slang` reads those. The two textures the specular extension
//! also defines are not read, and [`warn_specular_textures`] is what says so.
//!
//! A module of its own rather than more of [`super`], which is already the
//! whole of the document walk: this is one material property and the tests
//! that pin it, and it has no reason to be read alongside the geometry.
//!
//! [`GpuMaterial::specular_f0`]: crcbl_shaders::mesh::GpuMaterial::specular_f0
//! [`GpuMaterial::specular_f90`]: crcbl_shaders::mesh::GpuMaterial::specular_f90

use std::path::Path;

use super::first_report;

/// The dielectric reflectance a material's specular lobe starts from and rises
/// to, as `GpuMaterial::specular_f0` and `GpuMaterial::specular_f90` want them.
///
/// **Absent extensions are their defaults**, and each specification says what
/// that is: `KHR_materials_ior`'s [`DEFAULT_IOR`], `KHR_materials_specular`'s
/// `specularFactor` of one and white `specularColorFactor`. A property an
/// extension object leaves out is defaulted by the `gltf` crate to the same
/// values. The `F90` is the specular factor alone, which is what that
/// extension's Implementation section makes it.
///
/// Only the factors: `specularTexture` and `specularColorTexture` are not read
/// — see [`warn_specular_textures`].
pub(super) fn dielectric_specular(material: &gltf::Material<'_>) -> ([f32; 3], f32) {
    let ior = material.ior().unwrap_or(DEFAULT_IOR);
    let (factor, color) = material.specular().map_or((1.0, [1.0; 3]), |specular| {
        (specular.specular_factor(), specular.specular_color_factor())
    });
    (dielectric_f0(ior, factor, color), factor)
}

/// `KHR_materials_ior`'s default index of refraction.
const DEFAULT_IOR: f32 = 1.5;

/// `KHR_materials_specular`'s `dielectric_f0`, from its "Interaction with other
/// extensions" section: `min(((ior - 1) / (ior + 1))^2 * specularColor, 1) *
/// specular`.
///
/// **The clamp is on the colour product and the factor comes after it**, where
/// the specification places it: the colour factor may exceed one, so a
/// reflectance above what the IOR gives is expressible, and the clamp is what
/// keeps it energy-conserving before the specular weight scales it. An IOR of
/// zero — `KHR_materials_ior`'s specular-glossiness compatibility mode, whose
/// effective IOR is infinite — gives a reflectance of one here with no special
/// case, which is what that mode asks for.
///
/// **Evaluated in double precision**, and that is what makes the defaults land
/// on `GpuMaterial::DIELECTRIC_F0` exactly: in single precision an IOR of
/// `1.5` squares to one step above that literal, and a default material would
/// shade a last bit off every picture drawn before the extension was read.
fn dielectric_f0(ior: f32, specular_factor: f32, specular_color: [f32; 3]) -> [f32; 3] {
    let ior = f64::from(ior);
    let ratio = (ior - 1.0) / (ior + 1.0);
    let reflectance = ratio * ratio;
    specular_color.map(|channel| {
        ((reflectance * f64::from(channel)).min(1.0) * f64::from(specular_factor)) as f32
    })
}

/// Say, once per asset, that a document's `KHR_materials_specular` textures
/// were not applied while its factors were.
///
/// The factors reach the row; a texture would be a page layer per material on
/// two more pages, which `docs/backlog.md` carries. The count is the document's
/// materials naming either texture, so the line says how much of the file
/// shades with the factors alone.
pub(super) fn warn_specular_textures(document: &gltf::Document, key: &Path) {
    let textured = document
        .materials()
        .filter_map(|material| material.specular())
        .filter(|specular| {
            specular.specular_texture().is_some() || specular.specular_color_texture().is_some()
        })
        .count();
    if textured > 0 && first_report(key, SPECULAR_TEXTURES) {
        crcbl_core::log::warn!(
            "{}: ignoring the {SPECULAR_TEXTURES} of {textured} material(s): this importer \
             applies their specularFactor and specularColorFactor and not the textures",
            key.display(),
        );
    }
}

/// What [`warn_specular_textures`] reports, and the name it is deduplicated
/// under beside the extension names.
const SPECULAR_TEXTURES: &str = "KHR_materials_specular textures";

#[cfg(test)]
mod tests {
    use crcbl_shaders::mesh::GpuMaterial;

    use super::*;
    use crate::gltf_fixture::{
        BIN_CHUNK_BUFFER, IMAGE_TEXELS, glb, import_glb, import_glb_bytes_as, png_bytes, replacing,
        textured_parts, triangle_json,
    };

    /// The fixture's one material with `extensions` spliced in beside its
    /// factors.
    fn with_material_extensions(extensions: &str) -> String {
        replacing(
            &triangle_json(BIN_CHUNK_BUFFER),
            "\"name\": \"paint\",",
            &format!("\"name\": \"paint\", \"extensions\": {{ {extensions} }},"),
        )
    }

    /// The row the fixture's one material imports as, with `extensions`.
    fn imported(extensions: &str) -> GpuMaterial {
        import_glb(&with_material_extensions(extensions))
            .expect("the fixture imports")
            .materials()[0]
    }

    /// **The reflectance is the specification's formula**, checked against
    /// values worked by hand rather than against a second copy of it.
    ///
    /// Each case exercises one part: an IOR of two is `(1/3)^2`, one ninth; an
    /// IOR of one is air against air and reflects nothing; the colour and the
    /// factor scale per channel; and an IOR of zero reflects everything, which
    /// is where the clamp on the colour product can be seen to come *before*
    /// the factor — a clamp after it would read `0.5` rather than `0.25` in the
    /// second channel.
    ///
    /// # Sabotage
    ///
    /// The clamp moved after the factor (`(reflectance * channel *
    /// factor).min(1.0)`): red on the last case with `left: [0.25, 1.0, 0.5]`
    /// against `right: [0.25, 0.5, 0.5]`.
    #[test]
    fn the_reflectance_is_the_specification_formula() {
        assert_eq!(
            dielectric_f0(2.0, 1.0, [1.0; 3]),
            [(1.0f64 / 9.0) as f32; 3]
        );
        assert_eq!(dielectric_f0(1.0, 1.0, [1.0; 3]), [0.0; 3]);
        assert_eq!(
            dielectric_f0(2.0, 0.5, [1.0, 0.5, 0.0]),
            [(1.0f64 / 18.0) as f32, (1.0f64 / 36.0) as f32, 0.0]
        );
        assert_eq!(
            dielectric_f0(0.0, 0.5, [0.5, 2.0, 1.0]),
            [0.25, 0.5, 0.5],
            "the colour product is clamped to one, and the factor applies after the clamp"
        );
    }

    /// **Every default together is exactly the constant the lobe used before
    /// the row carried one** — the value `GpuMaterial::UNTINTED` holds, and
    /// the one a document naming neither extension imports as.
    ///
    /// # Sabotage
    ///
    /// `dielectric_f0` evaluated in `f32`: red with the bits `left:
    /// [1025758987, …]` against `right: [1025758986, …]` — one step above the
    /// literal, which is the rounding `GpuMaterial::DIELECTRIC_F0` documents.
    /// Every other test in this module went red with it too.
    #[test]
    fn the_defaults_reduce_to_the_constant_the_lobe_used() {
        assert_eq!(
            dielectric_f0(DEFAULT_IOR, 1.0, [1.0; 3]).map(f32::to_bits),
            [GpuMaterial::DIELECTRIC_F0.to_bits(); 3]
        );
        let plain = import_glb(&triangle_json(BIN_CHUNK_BUFFER))
            .expect("the fixture imports")
            .materials()[0];
        assert_eq!(plain.specular_f0, GpuMaterial::UNTINTED.specular_f0);
        assert_eq!(plain.specular_f90, GpuMaterial::UNTINTED.specular_f90);
        // An extension object that writes nothing is its defaults too.
        let empty = imported(r#""KHR_materials_ior": {}, "KHR_materials_specular": {}"#);
        assert_eq!(empty.specular_f0, GpuMaterial::UNTINTED.specular_f0);
        assert_eq!(empty.specular_f90, GpuMaterial::UNTINTED.specular_f90);
    }

    /// **An authored IOR reaches the row.**
    ///
    /// # Sabotage
    ///
    /// `dielectric_specular` reading `DEFAULT_IOR` in place of
    /// `material.ior()`: red with `left: [0.04, 0.04, 0.04]` against
    /// `right: [0.11111111, 0.11111111, 0.11111111]`.
    #[test]
    fn an_authored_ior_reaches_the_row() {
        let row = imported(r#""KHR_materials_ior": { "ior": 2.0 }"#);
        assert_eq!(row.specular_f0, [(1.0f64 / 9.0) as f32; 3]);
        assert_eq!(
            row.specular_f90, 1.0,
            "the IOR does not move the grazing end"
        );
    }

    /// **An authored specular factor reaches the row, at both ends.**
    ///
    /// # Sabotage
    ///
    /// `dielectric_specular` returning `1.0` for the `F90` in place of the
    /// factor: red with `left: 1.0` against `right: 0.5`, here and in
    /// `a_specular_texture_is_reported_once_and_its_factors_still_apply`. The
    /// factor dropped
    /// from `dielectric_f0` instead: red with `left: [0.04, 0.04, 0.04]`
    /// against `right: [0.02, 0.02, 0.02]`.
    #[test]
    fn an_authored_specular_factor_reaches_the_row() {
        let row = imported(r#""KHR_materials_specular": { "specularFactor": 0.5 }"#);
        assert_eq!(row.specular_f0, [0.02; 3]);
        assert_eq!(row.specular_f90, 0.5);
    }

    /// **An authored specular colour reaches the row**, channel for channel.
    ///
    /// # Sabotage
    ///
    /// `dielectric_specular` passing `[1.0; 3]` for the colour: red with
    /// `left: [0.04, 0.04, 0.04]` against `right: [0.04, 0.02, 0.01]`.
    #[test]
    fn an_authored_specular_colour_reaches_the_row() {
        let row =
            imported(r#""KHR_materials_specular": { "specularColorFactor": [1.0, 0.5, 0.25] }"#);
        assert_eq!(row.specular_f0, [0.04, 0.02, 0.01]);
        assert_eq!(row.specular_f90, 1.0);
    }

    /// **A specular texture is named in a warning, once per asset, and the
    /// factors beside it still reach the row.**
    ///
    /// # Sabotage
    ///
    /// `warn_specular_textures`' call removed from `warn_dropped_features`: red
    /// with "the first import did not say its specular texture was ignored".
    /// `first_report` returning `true` unconditionally: red with "the second
    /// import of the same asset repeated the line".
    #[test]
    fn a_specular_texture_is_reported_once_and_its_factors_still_apply() {
        let (base, bin) = textured_parts(&png_bytes(2, 2, &IMAGE_TEXELS), "image/png", 0);
        let json = replacing(
            &base,
            "\"name\": \"painted\",",
            "\"name\": \"painted\", \"extensions\": { \"KHR_materials_specular\": \
             { \"specularFactor\": 0.5, \"specularColorTexture\": { \"index\": 0 } } },",
        );
        let bytes = glb(&json, Some(&bin));
        let key = "meshes/specular-textured.glb";
        let lines = || {
            let logs = crcbl_core::log::capture();
            let scene = import_glb_bytes_as(&bytes, key).expect("the document imports");
            let lines: Vec<String> = logs
                .records()
                .into_iter()
                .filter(|record| record.message.contains(SPECULAR_TEXTURES))
                .map(|record| record.message)
                .collect();
            (scene, lines)
        };

        let (scene, first) = lines();
        assert_eq!(
            first.len(),
            1,
            "the first import did not say its specular texture was ignored: {first:#?}"
        );
        assert!(
            first[0].contains("1 material(s)"),
            "the line does not count the material: {}",
            first[0]
        );
        assert_eq!(
            scene.materials()[0].specular_f90,
            0.5,
            "the factor beside the ignored texture must still reach the row"
        );

        let (_, second) = lines();
        assert!(
            second.is_empty(),
            "the second import of the same asset repeated the line: {second:#?}"
        );
    }
}
