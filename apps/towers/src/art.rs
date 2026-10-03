//! The build menu's pixel art: an icon per tower kind and one for the upgrade,
//! registered into the UI pass's image atlas.
//!
//! ```text
//!   assets/icons.crpix ──build.rs──▶ baked sheet ──Icons::register──▶ ImageAtlas
//!                                                                      │
//!                                  crate::build_menu draws them ◀──────┘
//! ```
//!
//! Rule 11's first `.crpix` in this sample, and the reason the build menu
//! waited for it: a menu of tower kinds is a menu of pictures. Each frame is
//! found **by name** — a kind's is [`tower::Kind::label`], the upgrade's is
//! [`UPGRADE_FRAME`] — so the sheet's order is the author's to change and a
//! frame renamed out from under a kind is a test failure rather than a tower
//! drawn as another.
//!
//! **The art is a placeholder, typed by hand** — `assets/icons.crpix` says so,
//! and `docs/backlog.md` records what an art pass would replace.

use crcbl::sprite::load::{Loaded, load_baked};
use crcbl::ui::image::{AtlasError, AtlasImage, ImageAtlas};

use crate::tower;

// `build.rs` writes this: an `ICONS_PNG` and an `ICONS_JSON` for
// `assets/icons.crpix`, and the `ART_TICK_HZ` they were baked at.
include!(concat!(env!("OUT_DIR"), "/art_data.rs"));

/// The upgrade's frame in `assets/icons.crpix`; each kind's is its label.
pub const UPGRADE_FRAME: &str = "upgrade";

/// How many pixels wide and tall an icon is drawn: twice its sixteen texels,
/// a whole multiple so the sharp-bilinear sampling keeps every texel square.
pub const ICON_PX: f32 = 32.0;

/// The icon sheet, decoded. Panics on a sheet that does not load, which is a
/// broken build rather than bad input — see [`load_baked`].
#[must_use]
pub fn sheet() -> Loaded {
    load_baked("icons", ICONS_PNG, ICONS_JSON, ART_TICK_HZ)
}

/// Where each icon is in the atlas it was registered into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icons {
    /// One per kind, in [`tower::ALL`]'s order.
    kinds: [AtlasImage; tower::KINDS],
    upgrade: AtlasImage,
}

impl Icons {
    /// Registers every icon into `images`.
    ///
    /// # Errors
    ///
    /// [`AtlasError`] when `images` has no room left for them.
    ///
    /// # Panics
    ///
    /// When the baked sheet is missing a frame this module names — a kind
    /// without an icon, or no upgrade — which is a bug in this repository.
    pub fn register(images: &mut ImageAtlas) -> Result<Self, AtlasError> {
        let art = sheet();
        let mut register = |name: &str| -> Result<AtlasImage, AtlasError> {
            let index = frame_index(&art, name)
                .unwrap_or_else(|| panic!("assets/icons.crpix has no frame named {name}"));
            let rect = art.sheet.frames[index].rect;
            let pixels = art
                .frame_pixels(index)
                .expect("the index was just found among the frames");
            images.register(rect.w, rect.h, &pixels)
        };
        let mut kinds = Vec::with_capacity(tower::KINDS);
        for kind in tower::ALL {
            kinds.push(register(kind.label())?);
        }
        Ok(Self {
            kinds: kinds.try_into().expect("one icon was registered per kind"),
            upgrade: register(UPGRADE_FRAME)?,
        })
    }

    /// The icon a build of `kind` is offered with.
    #[must_use]
    pub const fn kind(&self, kind: tower::Kind) -> AtlasImage {
        self.kinds[kind.index()]
    }

    /// The icon an upgrade is offered with.
    #[must_use]
    pub const fn upgrade(&self) -> AtlasImage {
        self.upgrade
    }
}

/// Where the frame named `name` is in `art`'s sheet.
fn frame_index(art: &Loaded, name: &str) -> Option<usize> {
    art.sheet.frames.iter().position(|frame| frame.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The opaque texels of frame `name`, as RGBA quadruples.
    fn opaque(art: &Loaded, name: &str) -> Vec<[u8; 4]> {
        let index = frame_index(art, name).expect("the frame is on the sheet");
        art.frame_pixels(index)
            .expect("the frame is on the sheet")
            .chunks(4)
            .filter(|rgba| rgba[3] > 0)
            .map(|rgba| [rgba[0], rgba[1], rgba[2], rgba[3]])
            .collect()
    }

    /// **The sheet decodes, every kind and the upgrade has a frame of its own,
    /// and no two are the same picture.** The opaque count is the anti-blank:
    /// a frame of nothing but `.` loads as happily as a drawn one.
    #[test]
    fn every_kind_and_the_upgrade_has_an_icon_of_its_own() {
        let art = sheet();
        let names: Vec<&str> = tower::ALL
            .iter()
            .map(|kind| kind.label())
            .chain([UPGRADE_FRAME])
            .collect();
        let mut seen = Vec::new();
        for name in &names {
            let index =
                frame_index(&art, name).unwrap_or_else(|| panic!("the sheet has no {name} frame"));
            let rect = art.sheet.frames[index].rect;
            assert_eq!(
                (rect.w as f32 * 2.0, rect.h as f32 * 2.0),
                (ICON_PX, ICON_PX),
                "{name} is not drawn at twice its texels",
            );
            let drawn = opaque(&art, name);
            assert!(drawn.len() > 16, "{name} is all but empty");
            assert!(!seen.contains(&drawn), "{name} is another icon's picture");
            seen.push(drawn);
        }
        assert_eq!(art.sheet.frames.len(), names.len(), "a frame no icon uses");
    }

    /// **Each kind is registered as its own frame**, in the atlas: the
    /// texels at a kind's place on the page are that kind's frame and no
    /// other's — which is what a sheet looked up by position rather than by
    /// name would get wrong the day the frames are reordered.
    #[test]
    fn each_kind_is_registered_from_its_own_frame() {
        let mut images = ImageAtlas::new();
        let icons = Icons::register(&mut images).expect("an empty page has room");
        let art = sheet();
        let page = crcbl::ui::image::PAGE_SIZE as usize;
        let read = |image: AtlasImage| {
            let mut texels = Vec::new();
            for row in 0..image.height as usize {
                let start = ((image.y as usize + row) * page + image.x as usize) * 4;
                texels.extend_from_slice(&images.pixels()[start..start + image.width as usize * 4]);
            }
            texels
        };
        for kind in tower::ALL {
            let index = frame_index(&art, kind.label()).expect("the kind's frame");
            assert_eq!(
                read(icons.kind(kind)),
                art.frame_pixels(index).expect("the kind's frame"),
                "the {} icon is not the {} frame",
                kind.label(),
                kind.label(),
            );
        }
        let index = frame_index(&art, UPGRADE_FRAME).expect("the upgrade's frame");
        assert_eq!(
            read(icons.upgrade()),
            art.frame_pixels(index).expect("the upgrade's frame")
        );
    }
}
