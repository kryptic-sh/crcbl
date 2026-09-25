//! The menu's art: a nine-sliced window frame, a three-state button skin, and
//! the scrim behind both — registered into the UI's image atlas.
//!
//! ```text
//!   assets/menu.crpix ──build.rs──▶ baked sheet ──menu_skin──▶ ImageAtlas
//!                                                               │
//!                                   crcbl_ui::MenuSkin ◀────────┘
//!                                          │
//!          Menu::render (tree + default.css) ──▶ DrawList (scrim, frames, text)
//! ```
//!
//! # Why the art is here and not in each sample
//!
//! **A menu is the one picture every sample draws and no sample owns.**
//! `apps/breakout`, `apps/flappy` and `apps/sandbox` cannot depend on each other
//! — nothing in the workspace makes one app a library for another, and nothing
//! should — so art authored under `apps/*/assets/` is art authored once per
//! game. For a ball or a pipe that is exactly right: they are the game. For the
//! window a game pauses into it is three times the drawing, three files to keep
//! in step, and three menus that look like three different engines.
//!
//! Two homes were considered and one was picked.
//!
//! * **A shared assets directory both build scripts read.** Rejected. It shares
//!   the `.crpix` and nothing else: each `build.rs` still bakes it, each
//!   `art.rs` still loads it, each game still writes the code that registers it
//!   — so the duplication that actually costs something is untouched, and two
//!   build scripts grow a `../../..` path out of their own package, which cargo
//!   does not track for rebuilds the way it tracks a package's own files. It
//!   also gives `crcbl`'s end-to-end suite nothing: that crate cannot see
//!   `apps/`, so the golden image would have to be taken of a *replica* of the
//!   menu rather than of the menu.
//! * **This crate.** Picked. It already depends on `crcbl-sprite` (which bakes
//!   and loads) and on `crcbl-ui` (which lays out and draws), and its
//!   [`UiRenderer`](crate::UiRenderer) owns the image atlas the art is
//!   registered into. Every sample already depends on it, and so does the
//!   end-to-end suite, which is what lets the golden image be taken of the
//!   **real** art rather than of a lookalike built in the test.
//!
//! What it costs: `crcbl-render` has a `build.rs` and a `png` decoder it would
//! not otherwise need. The decoder is the same one every sample already links,
//! so no binary gained a dependency.
//!
//! # One sheet, five images
//!
//! All five frames — the window, the three button states and the scrim — are in
//! `assets/menu.crpix`, which is one file to author and one bake. They are
//! registered as **five images** rather than one, so each gets its own gutter
//! in the atlas and no frame's sampling can reach into its neighbour's.
//!
//! # Drawn by the UI pass
//!
//! The menu used to be a sprite pass of its own, with a screen-space camera,
//! sandwiched between the two halves of the draw list, because the UI pass had
//! no textured quad. It has one now: [`crcbl_ui::menu::Menu::render`] builds
//! the menu on the element tree, binds these images under the names
//! `crcbl-ui`'s `default.css` draws the frames with, and emits the scrim, each
//! frame and the text on it in paint order.

use crcbl_sprite::load::{Loaded, load_baked};
use crcbl_ui::image::{AtlasError, AtlasImage, ImageAtlas, NineSliceImage};
use crcbl_ui::menu::MenuSkin;
use crcbl_ui::{ButtonSkin, SkinInsets};

// `build.rs` writes this: one `*_PNG` and one `*_JSON` per `assets/*.crpix`.
include!(concat!(env!("OUT_DIR"), "/menu_art.rs"));

/// Which frame of `assets/menu.crpix` is which.
///
/// Named rather than spelled as literals at the places that index the sheet,
/// because a frame inserted in the middle would otherwise re-point the buttons
/// silently.
const PANEL_FRAME: usize = 0;
/// See [`PANEL_FRAME`].
const IDLE_FRAME: usize = 1;
/// See [`PANEL_FRAME`].
const HOVERED_FRAME: usize = 2;
/// See [`PANEL_FRAME`].
const PRESSED_FRAME: usize = 3;
/// See [`PANEL_FRAME`].
const SCRIM_FRAME: usize = 4;

/// Registers the shipped menu art into `images` and returns the skin
/// [`Menu::render`](crcbl_ui::menu::Menu::render) draws with.
///
/// Every frame shares the sheet's single `nine`, which is why the panel and the
/// buttons are both `4 4 4 4`: a [`Sheet`](crcbl_sprite::Sheet) carries one set
/// of insets for all its frames. Those are the texel insets each
/// [`NineSliceImage`] carries.
///
/// # Errors
///
/// [`AtlasError`] if `images` has no room for the five frames. Nothing here
/// fails on the *art*: it was parsed, validated and baked by `build.rs`.
///
/// # Panics
///
/// If the baked sheet cannot be read back, or does not declare the frames and
/// insets this module names — a bug in this repository rather than a runtime
/// condition.
pub fn menu_skin(images: &mut ImageAtlas) -> Result<MenuSkin, AtlasError> {
    let art = load_baked("menu", MENU_PNG, MENU_JSON, ART_TICK_HZ);
    let nine = art
        .sheet
        .nine
        .expect("menu.crpix declares a nine-slice over its frames");
    let insets = SkinInsets::new(
        nine.left as f32,
        nine.right as f32,
        nine.top as f32,
        nine.bottom as f32,
    );
    let mut register = |index: usize| -> Result<AtlasImage, AtlasError> {
        let rect = art
            .sheet
            .frames
            .get(index)
            .expect("menu.crpix has the five frames this module names")
            .rect;
        images.register(rect.w, rect.h, &frame_pixels(&art, index))
    };
    let panel = register(PANEL_FRAME)?;
    let idle = register(IDLE_FRAME)?;
    let hovered = register(HOVERED_FRAME)?;
    let pressed = register(PRESSED_FRAME)?;
    let scrim = register(SCRIM_FRAME)?;
    let sliced = |image| NineSliceImage { image, insets };
    Ok(MenuSkin {
        panel: sliced(panel),
        buttons: ButtonSkin {
            idle: sliced(idle),
            hovered: sliced(hovered),
            pressed: sliced(pressed),
        },
        scrim,
    })
}

/// Frame `index` of a loaded sheet, as its own block of RGBA cut out of the
/// strip, rows top to bottom.
fn frame_pixels(loaded: &Loaded, index: usize) -> Vec<u8> {
    let rect = loaded.sheet.frames[index].rect;
    let stride = loaded.image.width as usize * 4;
    (0..rect.h as usize)
        .flat_map(|row| {
            let start = (rect.y as usize + row) * stride + rect.x as usize * 4;
            loaded.image.pixels[start..start + rect.w as usize * 4].to_vec()
        })
        .collect()
}

/// Decodes the baked sheet at *this crate's* bake rate, for the tests below.
///
/// [`ART_TICK_HZ`] is generated into each crate that bakes art, so the rate is
/// per-crate configuration rather than something the engine can supply; the
/// failure policy is the shared half and lives in [`load_baked`].
#[cfg(test)]
fn baked(name: &str, png: &[u8], json: Option<&str>) -> Loaded {
    load_baked(name, png, json, ART_TICK_HZ)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_sprite::NineSlice;

    /// The [`Sheet`](crcbl_sprite::Sheet) a baked pair describes.
    fn sheet_of(loaded: &Loaded) -> &crcbl_sprite::Sheet {
        &loaded.sheet
    }

    // -----------------------------------------------------------------------
    // The art itself
    // -----------------------------------------------------------------------

    /// **The art is the art that was authored.** Sizes, frame names, holds and
    /// the nine-slice — every one a number written in `assets/menu.crpix` and
    /// carried through parse, bake and load.
    ///
    /// The alpha counts at the end are the anti-blank: a test that only checked
    /// `load` returned `Ok` would pass on an empty image.
    #[test]
    fn the_shipped_art_bakes_to_the_sheet_it_declares() {
        let art = baked("menu", MENU_PNG, MENU_JSON);
        assert_eq!(
            (art.image.width, art.image.height),
            (16 * 5, 16),
            "five 16x16 frames in one horizontal strip"
        );
        let names: Vec<&str> = sheet_of(&art)
            .frames
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(names, ["panel", "idle", "hovered", "pressed", "scrim"]);
        assert_eq!(sheet_of(&art).nine, Some(NineSlice::new(4, 4, 4, 4)));
        for (index, frame) in sheet_of(&art).frames.iter().enumerate() {
            assert_eq!(
                frame.rect,
                crcbl_sprite::Rect::new(index as u32 * 16, 0, 16, 16),
                "frame {index} is not a 16x16 cell of the strip"
            );
            assert_eq!(
                frame.hold, 1,
                "the default hold in ticks did not survive the millisecond round \
                 trip, so bake and load disagree about milliseconds"
            );
        }
        assert!(
            sheet_of(&art).clips.is_empty(),
            "the button frames are states, not a clip: a clip here would animate \
             a button nobody is touching"
        );

        // Anti-blank. Every frame is opaque everywhere — a menu you can see the
        // game through is the bug the scrim exists to prevent — and the scrim is
        // opaque white, which is what makes `Sprite::tint` the only thing that
        // decides how dark it gets.
        let clear = art
            .image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 0)
            .count();
        assert_eq!(clear, 0, "the menu art has holes in it");
        for y in 0..16usize {
            for x in 0..16usize {
                let i = ((y * 80) + SCRIM_FRAME * 16 + x) * 4;
                assert_eq!(
                    &art.image.pixels[i..i + 4],
                    [255, 255, 255, 255],
                    "the scrim is not white at ({x}, {y})"
                );
            }
        }
    }

    /// **Every stretched band is one colour along its whole span**, on both
    /// axes and in every frame.
    ///
    /// The property that makes a nine-slice a frame rather than a smear: the
    /// centre and the four edges are stretched, so a band whose span is not
    /// uniform comes out as a gradient at any size but the sheet's own. Nothing
    /// else in this file would notice — the quads would still be the right size
    /// and the corners still fixed.
    #[test]
    fn every_stretched_band_is_uniform_along_its_span() {
        let art = baked("menu", MENU_PNG, MENU_JSON);
        let texel = |frame: usize, x: usize, y: usize| -> [u8; 4] {
            let i = ((y * 80) + frame * 16 + x) * 4;
            art.image.pixels[i..i + 4]
                .try_into()
                .expect("four channels")
        };
        for frame in 0..5 {
            for y in 0..16 {
                let first = texel(frame, 4, y);
                for x in 5..12 {
                    assert_eq!(
                        texel(frame, x, y),
                        first,
                        "frame {frame} row {y} is not uniform across its stretched \
                         columns, so a widened panel smears it"
                    );
                }
            }
            for x in 0..16 {
                let first = texel(frame, x, 4);
                for y in 5..12 {
                    assert_eq!(
                        texel(frame, x, y),
                        first,
                        "frame {frame} column {x} is not uniform down its stretched \
                         rows"
                    );
                }
            }
        }
    }

    /// **The three button frames are three different pictures.**
    ///
    /// Three identical frames parse, bake, load and index exactly as three
    /// different ones do, and every other test here passes on them — so "the
    /// states draw different frames" is worth nothing until the frames are known
    /// to differ.
    #[test]
    fn the_three_button_frames_are_actually_different_pictures() {
        let art = baked("menu", MENU_PNG, MENU_JSON);
        let frames: Vec<Vec<u8>> = [IDLE_FRAME, HOVERED_FRAME, PRESSED_FRAME]
            .iter()
            .map(|i| frame_pixels(&art, *i))
            .collect();
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(frame.len(), 16 * 16 * 4, "frame {index} is not 16x16");
        }
        for a in 0..3 {
            for b in (a + 1)..3 {
                assert_ne!(
                    frames[a], frames[b],
                    "button frames {a} and {b} are the same picture, so pressing \
                     the button would change nothing on screen",
                );
            }
        }

        // And each is bevelled rather than flat: the row under the cap and the
        // row above the base differ, which is what a tint could not have given.
        for (index, frame) in frames.iter().enumerate() {
            let row = |r: usize| frame[r * 16 * 4..(r + 1) * 16 * 4].to_vec();
            assert_ne!(row(1), row(14), "frame {index} is a flat rectangle");
        }
        // And `pressed` is not `idle` darkened: its highlight is at the
        // *bottom*, which is the whole reason there are three frames and not one
        // frame and three tints. Compare each frame's top shading row against its
        // own bottom one — a uniform darkening leaves that relationship intact
        // and an inverted bevel swaps it.
        let brightness = |frame: &[u8], r: usize| -> u32 {
            frame[r * 16 * 4..(r + 1) * 16 * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]))
                .sum()
        };
        assert!(
            brightness(&frames[0], 1) > brightness(&frames[0], 14),
            "idle is not lit from the top",
        );
        assert!(
            brightness(&frames[2], 1) < brightness(&frames[2], 14),
            "pressed is idle darkened rather than idle inverted",
        );
    }

    /// **The layout and the art agree about the corners.**
    ///
    /// `crcbl_ui::menu` lays a label out inside bands this art refuses to
    /// stretch, and it cannot see the art to ask. The two constants are the
    /// joint; this reads the art's own insets back and compares. Without it a
    /// redrawn frame with a deeper base would put every label over its own
    /// shadow, and nothing would fail.
    #[test]
    fn the_shipped_art_has_the_insets_the_layout_assumes() {
        let skin = menu_skin(&mut ImageAtlas::new()).expect("five frames fit an empty page");
        assert_eq!(
            skin.panel.clamped_insets(),
            crcbl_ui::menu::PANEL_INSETS,
            "menu.crpix and crcbl_ui::menu::PANEL_INSETS disagree",
        );
        assert_eq!(
            skin.buttons.insets(),
            crcbl_ui::menu::BUTTON_INSETS,
            "menu.crpix and crcbl_ui::menu::BUTTON_INSETS disagree",
        );
        assert!(
            skin.buttons.insets_agree(),
            "the three button frames must share their insets",
        );
    }

    /// **The skin is the art: each registered image holds exactly its frame's
    /// texels, and each is a different frame.** A skin that registered one frame
    /// five times, or cut the strip at the wrong offset, draws a plausible
    /// window with the wrong picture in it.
    #[test]
    fn each_registered_image_is_its_own_frame_of_the_sheet() {
        let art = baked("menu", MENU_PNG, MENU_JSON);
        let mut images = ImageAtlas::new();
        let skin = menu_skin(&mut images).expect("fits");
        let page = crcbl_ui::image::PAGE_SIZE as usize;
        for (index, image) in [
            (PANEL_FRAME, skin.panel.image),
            (IDLE_FRAME, skin.buttons.idle.image),
            (HOVERED_FRAME, skin.buttons.hovered.image),
            (PRESSED_FRAME, skin.buttons.pressed.image),
            (SCRIM_FRAME, skin.scrim),
        ] {
            assert_eq!((image.width, image.height), (16, 16), "frame {index}");
            let expected = frame_pixels(&art, index);
            let mut actual = Vec::with_capacity(expected.len());
            for row in 0..16 {
                let start = ((image.y as usize + row) * page + image.x as usize) * 4;
                actual.extend_from_slice(&images.pixels()[start..start + 16 * 4]);
            }
            assert_eq!(actual, expected, "frame {index} is not what was registered");
        }
        let ids = [
            skin.panel.image.id,
            skin.buttons.idle.image.id,
            skin.buttons.hovered.image.id,
            skin.buttons.pressed.image.id,
            skin.scrim.id,
        ];
        for a in 0..ids.len() {
            for b in (a + 1)..ids.len() {
                assert_ne!(ids[a], ids[b], "two frames share one image");
            }
        }
    }
}
