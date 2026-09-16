//! The two layers as ordinary assets.
//!
//! `docs/plan/56-wind.md`'s decision 1: "Both are ordinary assets in a standard
//! image format, authored or cooked from terrain, loaded through
//! `crcbl-assets`." So there is no wind container format and no wind importer —
//! a layer is a PNG, read through the one IO seam
//! ([`crcbl_assets::AssetSource`]) and decoded by the one PNG reader this
//! workspace already has ([`crcbl_sprite::load::decode_png`]).
//!
//! # Polling, not blocking
//!
//! An [`crcbl_assets::AssetSource`] never blocks — the browser has no blocking
//! filesystem — so a source that does not have the bytes yet answers
//! [`crcbl_assets::StorageError::Pending`] and the caller polls again. Both
//! functions here pass that straight through as
//! [`WindError::Asset`]; see that variant for why it is not a state of its own.

use std::path::Path;

use crcbl_assets::AssetSource;
use crcbl_sprite::load::decode_png;

use crate::WindError;
use crate::layers::{DirectionLayer, IntensityLayer, LayerGrid};

/// Loads the coarse direction layer from `key`.
///
/// The image's own width and height set the grid; `metres_per_texel` is the
/// only thing the file does not carry, because a PNG has no world scale. The
/// research decision 1 cites suggests 8–16 m per texel for this layer.
///
/// # Errors
///
/// [`WindError::Asset`] if the source cannot hand over the bytes — including
/// [`crcbl_assets::StorageError::Pending`], which is a poll and not a failure;
/// [`WindError::Decode`] if they are not a readable PNG; and
/// [`WindError::EmptyLayer`], [`WindError::TexelScale`] or
/// [`WindError::LayerSize`] if the image cannot make a grid.
pub fn load_direction_layer(
    source: &dyn AssetSource,
    key: &Path,
    metres_per_texel: f64,
) -> Result<DirectionLayer, WindError> {
    let (grid, pixels) = read_layer(source, key, metres_per_texel)?;
    DirectionLayer::from_rgba8(grid, &pixels)
}

/// Loads the fine intensity layer from `key`.
///
/// As [`load_direction_layer`]; the research decision 1 cites suggests 1–2 m
/// per texel for this one.
///
/// # Errors
///
/// The same set [`load_direction_layer`] returns.
pub fn load_intensity_layer(
    source: &dyn AssetSource,
    key: &Path,
    metres_per_texel: f64,
) -> Result<IntensityLayer, WindError> {
    let (grid, pixels) = read_layer(source, key, metres_per_texel)?;
    IntensityLayer::from_rgba8(grid, &pixels)
}

/// Reads one layer's bytes and decodes them to RGBA8 on a grid.
///
/// The half both layers share: which asset, which decoder, and how a key that
/// went wrong is named in the error. What differs between them is only which
/// channels the texel is read out of, which is each layer type's own business.
fn read_layer(
    source: &dyn AssetSource,
    key: &Path,
    metres_per_texel: f64,
) -> Result<(LayerGrid, Vec<u8>), WindError> {
    let named = || key.display().to_string();
    let bytes = source.read(key).map_err(|source| WindError::Asset {
        key: named(),
        source,
    })?;
    let image = decode_png(&bytes).map_err(|source| WindError::Decode {
        key: named(),
        source,
    })?;
    let grid = LayerGrid::new(image.width, image.height, metres_per_texel)?;
    Ok((grid, image.pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_assets::{MemorySource, StorageError};

    /// The committed pair, as the bytes an asset source would hand over.
    ///
    /// `include_bytes!` **in the test only**: at runtime these are assets a
    /// game ships beside its other art and fetches over HTTP in a browser, so
    /// nothing in the library embeds them and the wasm binary carries none of
    /// them.
    fn committed() -> MemorySource {
        let mut source = MemorySource::new();
        source
            .insert(
                Path::new("wind/direction.png"),
                include_bytes!("../assets/direction.png").to_vec(),
            )
            .expect("a legal asset key");
        source
            .insert(
                Path::new("wind/intensity.png"),
                include_bytes!("../assets/intensity.png").to_vec(),
            )
            .expect("a legal asset key");
        source
    }

    #[test]
    fn the_committed_pair_loads_at_the_sizes_its_generator_wrote() {
        let source = committed();
        let direction =
            load_direction_layer(&source, Path::new("wind/direction.png"), 8.0).expect("a layer");
        let intensity =
            load_intensity_layer(&source, Path::new("wind/intensity.png"), 2.0).expect("a layer");
        assert_eq!(direction.grid().width, 32);
        assert_eq!(direction.grid().height, 32);
        assert_eq!(direction.grid().metres_per_texel(), 8.0);
        assert_eq!(intensity.grid().width, 64);
        assert_eq!(intensity.grid().height, 64);
        assert_eq!(intensity.grid().metres_per_texel(), 2.0);
    }

    #[test]
    fn a_missing_layer_is_named_in_the_error() {
        let source = committed();
        let error = load_intensity_layer(&source, Path::new("wind/nothing.png"), 2.0)
            .expect_err("the source does not have it");
        assert!(matches!(
            error,
            WindError::Asset {
                source: StorageError::NotFound(_),
                ..
            }
        ));
        assert!(
            error.to_string().contains("wind/nothing.png"),
            "the message names the key: {error}"
        );
    }

    #[test]
    fn bytes_that_are_not_an_image_are_refused_rather_than_read() {
        let mut source = MemorySource::new();
        source
            .insert(Path::new("wind/direction.png"), b"not a png".to_vec())
            .expect("a legal asset key");
        let error = load_direction_layer(&source, Path::new("wind/direction.png"), 8.0)
            .expect_err("nine bytes are not a PNG");
        assert!(matches!(error, WindError::Decode { .. }), "{error}");
    }

    #[test]
    fn a_layer_with_no_world_scale_is_refused() {
        let source = committed();
        assert!(matches!(
            load_intensity_layer(&source, Path::new("wind/intensity.png"), 0.0),
            Err(WindError::TexelScale { .. })
        ));
    }
}
