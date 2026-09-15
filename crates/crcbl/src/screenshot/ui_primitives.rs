//! [`Scene::UiPrimitives`](super::Scene::UiPrimitives)'s content:
//! `docs/plan/07-ui-debug.md` rung 1's draw-list primitives, laid out so each
//! one can be held to a relation read off its own pixels.
//!
//! A module of its own rather than more of `screenshot.rs`, which is already the
//! largest file in this crate: the layout, the two pictures and the draw list
//! are here, and the parent only names the variant and its build arm.
//!
//! # The layout, and what each part of it is for
//!
//! ```text
//!   ╭──────────╮  ╭─────────────╮     top row: rounded rectangles
//!   │  filled  │  │  bordered   │     one uniform radius, one per-corner
//!   ╰──────────╯  ╰─────────────╯
//!   ┌╌╌╌╌╌╌╌╌╌╌┐  ┌──┬──────┬──┐     bottom row, left: a checker image drawn
//!   ┊ ┌──────┐ ┊  │TL│ edge │TR│     bigger than the clip it is pushed under;
//!   ┊ │clip  │ ┊  ├──┼──────┼──┤     right: a nine-slice with a different
//!   ┊ └──────┘ ┊  │BL│ edge │BR│     colour in each corner
//!   └╌╌╌╌╌╌╌╌╌╌┘  └──┴──────┴──┘
//! ```
//!
//! * **The filled rectangle's corners are wide enough to blend.** A sixteen
//!   pixel radius puts several fragments on the arc, which is where a signed
//!   distance with no multisampling has to produce coverage between the fill
//!   and the clear.
//! * **The bordered one has two radii**, large on one diagonal and small on
//!   the other, so a corner the shader rounded with the wrong radius reads as a
//!   pixel present on one side and missing on the other.
//! * **The image quad overhangs its clip on every side**, so a clip that was
//!   not applied, or applied on one axis, paints the overhang.
//! * **The nine-slice's corners are four different colours** and its bands a
//!   fifth, so a stretched, mirrored or transposed corner is a wrong colour at
//!   a named pixel.
//!
//! Everything is laid out at [`UI_PRIMITIVES_BASE`] and scaled by a whole number
//! of pixels, so every edge, every clip and every nine-slice band stays on the
//! pixel grid at every `--size` that fits it.

use glam::Vec2;

use crate::ui::draw_list::{Border, CornerRadii, DrawList};
use crate::ui::image::{AtlasError, AtlasImage, ImageAtlas, NineSliceImage};
use crate::ui::widget::SkinInsets;

/// The extent the layout is written at. A frame at a whole multiple of it on
/// both axes draws the same picture that many times larger.
pub const UI_PRIMITIVES_BASE: (u32, u32) = (256, 192);

/// The filled rounded rectangle's colour, in linear light.
pub const UI_PRIMITIVES_FILL: [f32; 4] = [0.95, 0.45, 0.10, 1.0];

/// The bordered rounded rectangle's fill, in linear light.
pub const UI_PRIMITIVES_BORDERED_FILL: [f32; 4] = [0.10, 0.30, 0.80, 1.0];

/// The bordered rounded rectangle's border, in linear light.
pub const UI_PRIMITIVES_BORDER_COLOR: [f32; 4] = [1.0, 0.85, 0.0, 1.0];

/// The checker image's two texel colours, as sRGB bytes.
pub const UI_PRIMITIVES_CHECKER: [[u8; 3]; 2] = [[230, 230, 230], [60, 60, 60]];

/// The checker image's size in texels.
pub const UI_PRIMITIVES_CHECKER_TEXELS: u32 = 4;

/// The nine-slice image's size in texels, and its inset on every side.
pub const UI_PRIMITIVES_NINE_TEXELS: u32 = 12;

/// See [`UI_PRIMITIVES_NINE_TEXELS`].
pub const UI_PRIMITIVES_NINE_INSET: u32 = 4;

/// The nine-slice's corner colours as sRGB bytes: top-left, top-right,
/// bottom-left, bottom-right.
pub const UI_PRIMITIVES_NINE_CORNERS: [[u8; 3]; 4] =
    [[220, 40, 40], [40, 200, 60], [40, 80, 220], [240, 240, 240]];

/// The nine-slice's four edge bands, as sRGB bytes.
pub const UI_PRIMITIVES_NINE_EDGE: [u8; 3] = [128, 128, 128];

/// The nine-slice's centre, as sRGB bytes.
pub const UI_PRIMITIVES_NINE_CENTRE: [u8; 3] = [30, 30, 40];

/// Where every primitive of the scene lands for one extent, in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiPrimitivesLayout {
    /// Pixels per base pixel: the largest whole number at which
    /// [`UI_PRIMITIVES_BASE`] fits the extent, and never below one.
    pub unit: f32,
    /// The filled rounded rectangle: `(min, max)`.
    pub rounded: (Vec2, Vec2),
    /// Its radius, at every corner.
    pub rounded_radius: f32,
    /// The bordered rounded rectangle: `(min, max)`.
    pub bordered: (Vec2, Vec2),
    /// Its corner radii.
    pub bordered_radii: CornerRadii,
    /// Its border's width.
    pub border_width: f32,
    /// The clip the checker image is drawn under.
    pub clip: (Vec2, Vec2),
    /// The checker image's own rectangle, larger than the clip on every side.
    pub checker: (Vec2, Vec2),
    /// The nine-slice's rectangle.
    pub nine: (Vec2, Vec2),
    /// Pixels per texel of the nine-slice's fixed bands.
    pub nine_scale: f32,
}

/// The layout of [`Scene::UiPrimitives`](super::Scene::UiPrimitives) in an
/// `extent`-sized frame.
#[must_use]
pub fn ui_primitives_layout(extent: (u32, u32)) -> UiPrimitivesLayout {
    let unit = (extent.0 / UI_PRIMITIVES_BASE.0)
        .min(extent.1 / UI_PRIMITIVES_BASE.1)
        .max(1) as f32;
    let rect =
        |x0: f32, y0: f32, x1: f32, y1: f32| (Vec2::new(x0, y0) * unit, Vec2::new(x1, y1) * unit);
    UiPrimitivesLayout {
        unit,
        rounded: rect(16.0, 16.0, 112.0, 80.0),
        rounded_radius: 16.0 * unit,
        bordered: rect(128.0, 16.0, 240.0, 80.0),
        bordered_radii: CornerRadii {
            top_left: 24.0 * unit,
            top_right: 8.0 * unit,
            bottom_right: 24.0 * unit,
            bottom_left: 8.0 * unit,
        },
        border_width: 4.0 * unit,
        clip: rect(24.0, 104.0, 104.0, 168.0),
        checker: rect(8.0, 92.0, 120.0, 188.0),
        nine: rect(136.0, 96.0, 232.0, 176.0),
        nine_scale: 2.0 * unit,
    }
}

/// The two pictures the scene draws, once registered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiPrimitivesImages {
    /// A [`UI_PRIMITIVES_CHECKER_TEXELS`]-square checker of
    /// [`UI_PRIMITIVES_CHECKER`]'s two colours.
    pub checker: AtlasImage,
    /// The nine-slice frame.
    pub nine: NineSliceImage,
}

/// Registers the scene's two pictures into `images`.
///
/// # Errors
///
/// [`AtlasError`] if the atlas has no room for them.
pub fn register_ui_primitives_images(
    images: &mut ImageAtlas,
) -> Result<UiPrimitivesImages, AtlasError> {
    let side = UI_PRIMITIVES_CHECKER_TEXELS;
    let mut checker = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let [r, g, b] = UI_PRIMITIVES_CHECKER[((x + y) % 2) as usize];
            checker.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let checker = images.register(side, side, &checker)?;

    let side = UI_PRIMITIVES_NINE_TEXELS;
    let inset = UI_PRIMITIVES_NINE_INSET;
    let band = |t: u32| {
        if t < inset {
            0
        } else if t < side - inset {
            1
        } else {
            2
        }
    };
    let mut frame = Vec::with_capacity((side * side * 4) as usize);
    for y in 0..side {
        for x in 0..side {
            let [r, g, b] = match (band(x), band(y)) {
                (0, 0) => UI_PRIMITIVES_NINE_CORNERS[0],
                (2, 0) => UI_PRIMITIVES_NINE_CORNERS[1],
                (0, 2) => UI_PRIMITIVES_NINE_CORNERS[2],
                (2, 2) => UI_PRIMITIVES_NINE_CORNERS[3],
                (1, 1) => UI_PRIMITIVES_NINE_CENTRE,
                _ => UI_PRIMITIVES_NINE_EDGE,
            };
            frame.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let nine = NineSliceImage {
        image: images.register(side, side, &frame)?,
        insets: SkinInsets::uniform(inset as f32),
    };
    Ok(UiPrimitivesImages { checker, nine })
}

/// The scene's draw list for an `extent`-sized frame, drawing `images`.
#[must_use]
pub fn ui_primitives_draw_list(extent: (u32, u32), images: &UiPrimitivesImages) -> DrawList {
    let layout = ui_primitives_layout(extent);
    let mut list = DrawList::new();
    list.rounded_rect(
        layout.rounded.0,
        layout.rounded.1,
        CornerRadii::uniform(layout.rounded_radius),
        UI_PRIMITIVES_FILL,
        Border::NONE,
    );
    list.rounded_rect(
        layout.bordered.0,
        layout.bordered.1,
        layout.bordered_radii,
        UI_PRIMITIVES_BORDERED_FILL,
        Border {
            width: layout.border_width,
            color: UI_PRIMITIVES_BORDER_COLOR,
        },
    );
    list.push_clip(layout.clip.0, layout.clip.1);
    list.image(
        layout.checker.0,
        layout.checker.1,
        &images.checker,
        [1.0; 4],
    );
    list.pop_clip()
        .expect("the clip pushed just above is the only one");
    list.nine_slice(
        layout.nine.0,
        layout.nine.1,
        &images.nine,
        layout.nine_scale,
        [1.0; 4],
    );
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw_list::DrawCommand;

    /// Everything the scene draws is inside the frame, the clip is strictly
    /// inside the image it clips, and each kind of primitive is there — at the
    /// base extent and at twice it.
    #[test]
    fn the_scene_draws_every_primitive_inside_the_frame() {
        let images = register_ui_primitives_images(&mut ImageAtlas::new()).expect("fits");
        for extent in [UI_PRIMITIVES_BASE, (512, 384), (600, 400)] {
            let layout = ui_primitives_layout(extent);
            let list = ui_primitives_draw_list(extent, &images);
            let (mut rounded, mut pictures) = (0, 0);
            for command in list.commands() {
                let (min, max) = match command {
                    DrawCommand::RoundedRect { min, max, .. } => {
                        rounded += 1;
                        (*min, *max)
                    }
                    DrawCommand::Image { min, max, .. } => {
                        pictures += 1;
                        (*min, *max)
                    }
                    other => panic!("{extent:?}: the scene drew {other:?}"),
                };
                assert!(
                    min.cmpge(Vec2::ZERO).all()
                        && max.cmple(Vec2::new(extent.0 as f32, extent.1 as f32)).all(),
                    "{extent:?}: {command:?} leaves the frame"
                );
            }
            assert_eq!((rounded, pictures), (2, 1 + 9), "{extent:?}");
            assert!(
                layout.checker.0.cmplt(layout.clip.0).all()
                    && layout.checker.1.cmpgt(layout.clip.1).all(),
                "{extent:?}: the checker does not overhang its clip on every side"
            );
            // The clip reached the image and nothing else.
            let clipped: Vec<bool> = list
                .clips()
                .iter()
                .map(|clip| *clip != crate::ui::draw_list::ClipRect::NONE)
                .collect();
            assert_eq!(clipped.iter().filter(|c| **c).count(), 1, "{extent:?}");
            assert!(
                clipped[2],
                "{extent:?}: the image is not the clipped command"
            );
        }
    }
}
