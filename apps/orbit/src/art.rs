//! The flight UI's pixel art: the navball-lite, the markers that ride it and
//! the map, and the panel's chrome — registered into the UI pass's image atlas.
//!
//! ```text
//!   assets/markers.crpix ─┐
//!   assets/navball.crpix ─┼─build.rs──▶ baked sheets ──Art::register──▶ ImageAtlas
//!   assets/chrome.crpix  ─┘                                              │
//!                                       crate::page draws them ◀─────────┘
//! ```
//!
//! Sample rule 11's `.crpix` for this sample, which `06-orbit.md`'s Scope asks
//! for by name: the bodies and the rocket's trajectory are the map's own
//! geometry, and the 2D layer over them is sprites. Every frame is found **by
//! name** — the names are the constants below — so a sheet's order is the
//! author's to change and a frame renamed out from under its constant is a
//! test failure rather than one marker drawn as another.
//!
//! **The art is a placeholder, written as text** — each sheet says so.

use crcbl::sprite::load::{Loaded, load_baked};
use crcbl::ui::image::{AtlasError, AtlasImage, ImageAtlas, NineSliceImage};
use crcbl::ui::widget::SkinInsets;

// `build.rs` writes this: a `<STEM>_PNG` and a `<STEM>_JSON` for each sheet in
// `assets/`, and the `ART_TICK_HZ` they were baked at.
include!(concat!(env!("OUT_DIR"), "/art_data.rs"));

/// The navball's prograde marker in `assets/markers.crpix`.
pub const PROGRADE_FRAME: &str = "prograde";
/// The navball's retrograde marker.
pub const RETROGRADE_FRAME: &str = "retrograde";
/// The navball's heading marker: which way the engine points.
pub const HEADING_FRAME: &str = "heading";
/// The map's apoapsis glyph.
pub const APOAPSIS_FRAME: &str = "apoapsis";
/// The map's periapsis glyph.
pub const PERIAPSIS_FRAME: &str = "periapsis";
/// The map's ship marker.
pub const SHIP_FRAME: &str = "ship";
/// Every frame `assets/markers.crpix` holds, in the order the sheet does.
pub const MARKER_FRAMES: [&str; 6] = [
    PROGRADE_FRAME,
    RETROGRADE_FRAME,
    HEADING_FRAME,
    APOAPSIS_FRAME,
    PERIAPSIS_FRAME,
    SHIP_FRAME,
];

/// The navball under an atmosphere, in `assets/navball.crpix`.
pub const AIR_FRAME: &str = "air";
/// The navball in vacuum.
pub const VACUUM_FRAME: &str = "vacuum";
/// Every frame `assets/navball.crpix` holds.
pub const NAVBALL_FRAMES: [&str; 2] = [AIR_FRAME, VACUUM_FRAME];

/// The instrument panel's window, in `assets/chrome.crpix`.
pub const PANEL_FRAME: &str = "panel";
/// A gauge's empty well.
pub const TRACK_FRAME: &str = "track";
/// The full part of a gauge, white so the draw's tint is its colour.
pub const FILL_FRAME: &str = "fill";
/// Every frame `assets/chrome.crpix` holds.
pub const CHROME_FRAMES: [&str; 3] = [PANEL_FRAME, TRACK_FRAME, FILL_FRAME];

/// How many screen pixels one texel of the markers and the navball is drawn
/// at: a whole multiple, so the sharp-bilinear sampling keeps every texel
/// square, and two because one leaves a marker too small to tell prograde from
/// retrograde at a glance.
pub const TEXEL_PX: f32 = 2.0;

/// How many pixels one texel of the nine-sliced chrome's fixed bands is drawn
/// at — see [`crcbl::ui::draw_list::DrawList::nine_slice`]. One, so the panel's
/// outline is the hairline it was before it was art.
pub const CHROME_SCALE: f32 = 1.0;

/// Where each frame landed in the atlas it was registered into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Art {
    /// See [`PROGRADE_FRAME`].
    pub prograde: AtlasImage,
    /// See [`RETROGRADE_FRAME`].
    pub retrograde: AtlasImage,
    /// See [`HEADING_FRAME`].
    pub heading: AtlasImage,
    /// See [`APOAPSIS_FRAME`].
    pub apoapsis: AtlasImage,
    /// See [`PERIAPSIS_FRAME`].
    pub periapsis: AtlasImage,
    /// See [`SHIP_FRAME`].
    pub ship: AtlasImage,
    /// See [`AIR_FRAME`].
    pub navball_air: AtlasImage,
    /// See [`VACUUM_FRAME`].
    pub navball_vacuum: AtlasImage,
    /// See [`PANEL_FRAME`].
    pub panel: NineSliceImage,
    /// See [`TRACK_FRAME`].
    pub track: NineSliceImage,
    /// See [`FILL_FRAME`].
    pub fill: NineSliceImage,
}

/// The markers sheet, decoded. Panics on a sheet that does not load, which is
/// a broken build rather than bad input — see [`load_baked`].
#[must_use]
pub fn markers() -> Loaded {
    load_baked("markers", MARKERS_PNG, MARKERS_JSON, ART_TICK_HZ)
}

/// The navball sheet, decoded; see [`markers`].
#[must_use]
pub fn navball() -> Loaded {
    load_baked("navball", NAVBALL_PNG, NAVBALL_JSON, ART_TICK_HZ)
}

/// The chrome sheet, decoded; see [`markers`].
#[must_use]
pub fn chrome() -> Loaded {
    load_baked("chrome", CHROME_PNG, CHROME_JSON, ART_TICK_HZ)
}

impl Art {
    /// Registers every frame into `images`.
    ///
    /// # Errors
    ///
    /// [`AtlasError`] when `images` has no room left for them.
    ///
    /// # Panics
    ///
    /// When a baked sheet is missing a frame this module names, or the chrome
    /// sheet declares no nine-slice — a bug in this repository, not a run-time
    /// condition.
    pub fn register(images: &mut ImageAtlas) -> Result<Self, AtlasError> {
        let markers = markers();
        let navball = navball();
        let chrome = chrome();
        let nine = chrome
            .sheet
            .nine
            .expect("assets/chrome.crpix declares a nine-slice over its frames");
        let insets = SkinInsets::new(
            nine.left as f32,
            nine.right as f32,
            nine.top as f32,
            nine.bottom as f32,
        );
        let mut sliced = |name: &str| -> Result<NineSliceImage, AtlasError> {
            Ok(NineSliceImage {
                image: register(images, &chrome, "chrome", name)?,
                insets,
            })
        };
        let panel = sliced(PANEL_FRAME)?;
        let track = sliced(TRACK_FRAME)?;
        let fill = sliced(FILL_FRAME)?;
        Ok(Self {
            prograde: register(images, &markers, "markers", PROGRADE_FRAME)?,
            retrograde: register(images, &markers, "markers", RETROGRADE_FRAME)?,
            heading: register(images, &markers, "markers", HEADING_FRAME)?,
            apoapsis: register(images, &markers, "markers", APOAPSIS_FRAME)?,
            periapsis: register(images, &markers, "markers", PERIAPSIS_FRAME)?,
            ship: register(images, &markers, "markers", SHIP_FRAME)?,
            navball_air: register(images, &navball, "navball", AIR_FRAME)?,
            navball_vacuum: register(images, &navball, "navball", VACUUM_FRAME)?,
            panel,
            track,
            fill,
        })
    }
}

/// Registers the frame named `name` of `art` — the sheet `assets/<sheet>.crpix`
/// baked to — into `images`.
fn register(
    images: &mut ImageAtlas,
    art: &Loaded,
    sheet: &str,
    name: &str,
) -> Result<AtlasImage, AtlasError> {
    let index = art
        .sheet
        .frame_index(name)
        .unwrap_or_else(|| panic!("assets/{sheet}.crpix has no frame named {name}"));
    let rect = art.sheet.frames[index].rect;
    let pixels = art
        .frame_pixels(index)
        .expect("the index was just found among the frames");
    images.register(rect.w, rect.h, &pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The opaque texels of frame `name`, as RGBA quadruples.
    fn opaque(art: &Loaded, name: &str) -> Vec<[u8; 4]> {
        let index = art
            .sheet
            .frame_index(name)
            .expect("the frame is on the sheet");
        art.frame_pixels(index)
            .expect("the frame is on the sheet")
            .chunks(4)
            .filter(|rgba| rgba[3] > 0)
            .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
            .collect()
    }

    /// **Every sheet decodes, every frame this module names is on it, and no
    /// two frames of a sheet are the same picture.** The opaque count is the
    /// anti-blank: a frame of nothing but `.` loads as happily as a drawn one.
    /// And a frame no constant names is a failure too, because it is art the
    /// page cannot be drawing.
    #[test]
    fn every_named_frame_is_on_its_sheet_and_drawn() {
        for (sheet, art, names) in [
            ("markers", markers(), MARKER_FRAMES.as_slice()),
            ("navball", navball(), NAVBALL_FRAMES.as_slice()),
            ("chrome", chrome(), CHROME_FRAMES.as_slice()),
        ] {
            let mut seen = Vec::new();
            for name in names {
                assert!(
                    art.sheet.frame_index(name).is_some(),
                    "{sheet} has no {name} frame"
                );
                let drawn = opaque(&art, name);
                let area = art.sheet.frame(name).expect("found above").rect;
                assert!(
                    drawn.len() * 8 > (area.w * area.h) as usize,
                    "{sheet}'s {name} is all but empty",
                );
                assert!(!seen.contains(&drawn), "{sheet}'s {name} is another frame");
                seen.push(drawn);
            }
            assert_eq!(
                art.sheet.frames.len(),
                names.len(),
                "{sheet}: a frame nothing draws"
            );
        }
    }

    /// **The chrome stretches as a frame, not a smear**: the sheet's
    /// nine-slice leaves a centre band, and every stretched band is one colour
    /// along its whole span in every frame — `assets/chrome.crpix`'s own rule.
    #[test]
    fn the_chrome_stretches_without_smearing() {
        let art = chrome();
        let nine = art.sheet.nine.expect("a nine-slice");
        for name in CHROME_FRAMES {
            let index = art.sheet.frame_index(name).expect("on the sheet");
            let rect = art.sheet.frames[index].rect;
            let pixels = art.frame_pixels(index).expect("on the sheet");
            let texel = |x: u32, y: u32| {
                let at = ((y * rect.w + x) * 4) as usize;
                [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
            };
            let (xs, ys) = (
                nine.left..rect.w - nine.right,
                nine.top..rect.h - nine.bottom,
            );
            assert!(
                !xs.is_empty() && !ys.is_empty(),
                "{name} has no centre band"
            );
            for y in 0..rect.h {
                for x in xs.clone() {
                    assert_eq!(texel(x, y), texel(xs.start, y), "{name} row {y} smears");
                }
            }
            for x in 0..rect.w {
                for y in ys.clone() {
                    assert_eq!(texel(x, y), texel(x, ys.start), "{name} column {x} smears");
                }
            }
        }
    }

    /// **Each frame is registered as itself**: the texels at an image's place
    /// on the atlas page are its own frame's and no other's — which is what a
    /// sheet looked up by position rather than by name would get wrong the day
    /// its frames are reordered.
    #[test]
    fn each_frame_is_registered_from_its_own_texels() {
        let mut images = ImageAtlas::new();
        let art = Art::register(&mut images).expect("an empty page has room");
        let page = crcbl::ui::image::PAGE_SIZE as usize;
        let read = |image: AtlasImage| {
            let mut texels = Vec::new();
            for row in 0..image.height as usize {
                let start = ((image.y as usize + row) * page + image.x as usize) * 4;
                texels.extend_from_slice(&images.pixels()[start..start + image.width as usize * 4]);
            }
            texels
        };
        let (markers, navball, chrome) = (markers(), navball(), chrome());
        for (image, sheet, name) in [
            (art.prograde, &markers, PROGRADE_FRAME),
            (art.retrograde, &markers, RETROGRADE_FRAME),
            (art.heading, &markers, HEADING_FRAME),
            (art.apoapsis, &markers, APOAPSIS_FRAME),
            (art.periapsis, &markers, PERIAPSIS_FRAME),
            (art.ship, &markers, SHIP_FRAME),
            (art.navball_air, &navball, AIR_FRAME),
            (art.navball_vacuum, &navball, VACUUM_FRAME),
            (art.panel.image, &chrome, PANEL_FRAME),
            (art.track.image, &chrome, TRACK_FRAME),
            (art.fill.image, &chrome, FILL_FRAME),
        ] {
            let index = sheet.sheet.frame_index(name).expect("on the sheet");
            assert_eq!(
                read(image),
                sheet.frame_pixels(index).expect("on the sheet"),
                "the {name} image is not the {name} frame",
            );
        }
    }
}
