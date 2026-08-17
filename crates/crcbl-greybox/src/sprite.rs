//! The 2D half of the greybox pack: committed `.crpix` prototyping sprites,
//! baked into the crate and loaded back as [`Loaded`] sheets.
//!
//! A blockout kit for a 2D game the way [`crate::primitives`] is for a 3D one:
//! tiles, slopes, ladders and props sized to a grid, plus magenta actor
//! placeholders to stand in for a player and an enemy. Every sprite is authored
//! by hand in `assets/<stem>.crpix`, baked to a PNG by `build.rs`, and read
//! back here with [`crcbl_sprite::load::load_baked`] — so the pack ships as
//! source text in the repository and a game links the pixels.
//!
//! # One tile is 32 texels — pick the scale with `TEXELS_PER_UNIT`
//!
//! There is no engine-wide pixels-per-unit: a game chooses its own
//! ([`crcbl_render`]'s samples use 10 for breakout and 20 for flappy). These
//! sprites are authored at a **32-texel base tile**, so
//! `TEXELS_PER_UNIT = 32` makes one tile one world unit — [`GreyboxSprite::GridTile`]
//! is then a 1×1-unit cell and [`GreyboxSprite::Player`] a 1×2-unit figure.
//! [`GreyboxSprite::texels`] returns each sprite's authored size, and dividing
//! it by the game's `TEXELS_PER_UNIT` is its size in world units.
//!
//! # Loading
//!
//! [`load`] decodes one sprite; [`GreyboxSprite::ALL`] is every primitive, for a
//! game that uploads the whole set at start-up. The sheets are single still
//! frames — nothing here animates — so each [`Loaded::sheet`] has one frame
//! covering the whole image, and [`GreyboxSprite::texels`] is that frame's size.

use crcbl_sprite::load::{Loaded, load_baked};

// `build.rs` writes this: `ART_TICK_HZ`, and one `pub(crate)` `*_PNG` and
// `*_JSON` per `assets/*.crpix`. The stills bake no sidecar, so every `*_JSON`
// here is `None` and the loader synthesises a one-frame sheet from the image.
include!(concat!(env!("OUT_DIR"), "/greybox_art.rs"));

/// The base tile size these sprites are authored at, in texels.
///
/// Set a game's own pixels-per-unit to this and one tile is one world unit; see
/// the module docs. It is the divisor a caller turns [`GreyboxSprite::texels`]
/// into world units with.
pub const TEXELS_PER_UNIT: u32 = 32;

/// One 2D greybox primitive.
///
/// The `.crpix` stem each maps to is `assets/<stem>.crpix`; the sizes are in
/// [`GreyboxSprite::texels`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GreyboxSprite {
    /// The scale tile: a bordered 32×32 cell with a centre mark.
    GridTile,
    /// A plain filled 32×32 tile with a one-texel border.
    Solid,
    /// A filled 32×32 disc on transparency.
    Circle,
    /// A 4×4 checkerboard, 32×32.
    Checker,
    /// A one-tile floor bar, 32×8.
    Platform,
    /// A one-way platform, 32×8: the bright top edge is the solid side.
    PlatformOneway,
    /// A 45° ramp, 32×32.
    Slope45,
    /// A shallow 2:1 ramp, 32×32.
    Slope2to1,
    /// A ladder — two rails and rungs — 32×32.
    Ladder,
    /// A row of upward spikes, 32×32.
    Spike,
    /// The scale figure: one tile wide, two tall (32×64), in the actor tint.
    Player,
    /// An actor-tint enemy placeholder, 32×32, with a silhouette distinct from
    /// the player's.
    Enemy,
    /// A small collectible, 16×16.
    Pickup,
    /// A right-pointing direction indicator, 32×32.
    Arrow,
}

impl GreyboxSprite {
    /// Every primitive, in `assets/` order — for uploading the whole set.
    pub const ALL: [GreyboxSprite; 14] = [
        GreyboxSprite::GridTile,
        GreyboxSprite::Solid,
        GreyboxSprite::Circle,
        GreyboxSprite::Checker,
        GreyboxSprite::Platform,
        GreyboxSprite::PlatformOneway,
        GreyboxSprite::Slope45,
        GreyboxSprite::Slope2to1,
        GreyboxSprite::Ladder,
        GreyboxSprite::Spike,
        GreyboxSprite::Player,
        GreyboxSprite::Enemy,
        GreyboxSprite::Pickup,
        GreyboxSprite::Arrow,
    ];

    /// The sprite's name, matching its `.crpix` stem.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            GreyboxSprite::GridTile => "grid_tile",
            GreyboxSprite::Solid => "solid",
            GreyboxSprite::Circle => "circle",
            GreyboxSprite::Checker => "checker",
            GreyboxSprite::Platform => "platform",
            GreyboxSprite::PlatformOneway => "platform_oneway",
            GreyboxSprite::Slope45 => "slope_45",
            GreyboxSprite::Slope2to1 => "slope_2to1",
            GreyboxSprite::Ladder => "ladder",
            GreyboxSprite::Spike => "spike",
            GreyboxSprite::Player => "player",
            GreyboxSprite::Enemy => "enemy",
            GreyboxSprite::Pickup => "pickup",
            GreyboxSprite::Arrow => "arrow",
        }
    }

    /// The sprite's authored size in texels, `(width, height)`.
    ///
    /// This is the frame size a game plans layout against, and — divided by its
    /// `TEXELS_PER_UNIT` — the sprite's size in world units. The crate's tests
    /// assert it against the baked sheet, so an edit to a `.crpix` that changes
    /// a size and forgets this pairing fails the build.
    #[must_use]
    pub const fn texels(self) -> (u32, u32) {
        match self {
            GreyboxSprite::Platform | GreyboxSprite::PlatformOneway => (32, 8),
            GreyboxSprite::Player => (32, 64),
            GreyboxSprite::Pickup => (16, 16),
            GreyboxSprite::GridTile
            | GreyboxSprite::Solid
            | GreyboxSprite::Circle
            | GreyboxSprite::Checker
            | GreyboxSprite::Slope45
            | GreyboxSprite::Slope2to1
            | GreyboxSprite::Ladder
            | GreyboxSprite::Spike
            | GreyboxSprite::Enemy
            | GreyboxSprite::Arrow => (32, 32),
        }
    }

    /// The baked bytes for this sprite: `(name, png, sidecar)`.
    ///
    /// The stills bake no sidecar, so `sidecar` is always `None` and the loader
    /// reads one frame covering the whole image.
    const fn art(self) -> (&'static str, &'static [u8], Option<&'static str>) {
        match self {
            GreyboxSprite::GridTile => ("grid_tile", GRID_TILE_PNG, GRID_TILE_JSON),
            GreyboxSprite::Solid => ("solid", SOLID_PNG, SOLID_JSON),
            GreyboxSprite::Circle => ("circle", CIRCLE_PNG, CIRCLE_JSON),
            GreyboxSprite::Checker => ("checker", CHECKER_PNG, CHECKER_JSON),
            GreyboxSprite::Platform => ("platform", PLATFORM_PNG, PLATFORM_JSON),
            GreyboxSprite::PlatformOneway => {
                ("platform_oneway", PLATFORM_ONEWAY_PNG, PLATFORM_ONEWAY_JSON)
            }
            GreyboxSprite::Slope45 => ("slope_45", SLOPE_45_PNG, SLOPE_45_JSON),
            GreyboxSprite::Slope2to1 => ("slope_2to1", SLOPE_2TO1_PNG, SLOPE_2TO1_JSON),
            GreyboxSprite::Ladder => ("ladder", LADDER_PNG, LADDER_JSON),
            GreyboxSprite::Spike => ("spike", SPIKE_PNG, SPIKE_JSON),
            GreyboxSprite::Player => ("player", PLAYER_PNG, PLAYER_JSON),
            GreyboxSprite::Enemy => ("enemy", ENEMY_PNG, ENEMY_JSON),
            GreyboxSprite::Pickup => ("pickup", PICKUP_PNG, PICKUP_JSON),
            GreyboxSprite::Arrow => ("arrow", ARROW_PNG, ARROW_JSON),
        }
    }
}

/// Decodes one baked greybox sprite.
///
/// The sheet and its RGBA8 image, ready for
/// [`crcbl_render::texture`] to upload. The art was
/// parsed, validated and baked by `build.rs`, so a sprite that will not load is
/// a bug in this repository rather than a runtime condition — [`load_baked`]
/// panics rather than returning a `Result` the caller would only `unwrap`.
///
/// # Panics
///
/// If the baked sheet cannot be read back, which means the bake and this module
/// disagree about what was written.
#[must_use]
pub fn load(kind: GreyboxSprite) -> Loaded {
    let (name, png, sidecar) = kind.art();
    load_baked(name, png, sidecar, ART_TICK_HZ)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every primitive loads, and its baked sheet is the size
    /// [`GreyboxSprite::texels`] documents.
    ///
    /// The sheet size comes from the baked PNG, which comes from the `.crpix`
    /// header — so this fails if a source file is redrawn at a new size without
    /// its `texels()` arm following, and if any `.crpix` stops parsing or
    /// baking at all.
    #[test]
    fn every_sprite_bakes_to_its_documented_texels() {
        for kind in GreyboxSprite::ALL {
            let loaded = load(kind);
            let (w, h) = kind.texels();
            assert_eq!(
                (loaded.sheet.width, loaded.sheet.height),
                (w, h),
                "{}'s baked sheet is not its documented size",
                kind.name(),
            );
            assert_eq!(
                (loaded.image.width, loaded.image.height),
                (w, h),
                "{}'s baked image is not its documented size",
                kind.name(),
            );
            // A still bakes one frame covering the whole picture.
            let frame = &loaded.sheet.frames[0];
            assert_eq!(
                (frame.rect.w, frame.rect.h),
                (w, h),
                "{}'s frame does not cover its sheet",
                kind.name(),
            );
        }
    }

    /// The sizes the pack promises, spelled out so a change to any one arm of
    /// [`GreyboxSprite::texels`] is a red test rather than a silent redraw.
    #[test]
    fn the_documented_texel_sizes() {
        assert_eq!(GreyboxSprite::GridTile.texels(), (32, 32));
        assert_eq!(GreyboxSprite::Player.texels(), (32, 64));
        assert_eq!(GreyboxSprite::Platform.texels(), (32, 8));
        assert_eq!(GreyboxSprite::PlatformOneway.texels(), (32, 8));
        assert_eq!(GreyboxSprite::Pickup.texels(), (16, 16));
        assert_eq!(GreyboxSprite::Enemy.texels(), (32, 32));
    }

    /// `ALL` is every variant once, in `assets/` order, so uploading the set
    /// misses nothing and loads no sprite twice.
    #[test]
    fn all_is_complete_and_unique() {
        assert_eq!(GreyboxSprite::ALL.len(), 14);
        for (index, kind) in GreyboxSprite::ALL.iter().enumerate() {
            assert!(
                !GreyboxSprite::ALL[..index].contains(kind),
                "{} appears twice in ALL",
                kind.name(),
            );
        }
    }
}
