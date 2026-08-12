//! The menu's art: a nine-sliced window frame, a three-state button skin, and
//! the scrim behind both.
//!
//! ```text
//!   crcbl_ui::MenuLayout            this module              sprite pass
//!   ────────────────────            ───────────              ───────────
//!   panel  (min, max) px  ─┐
//!   button (min, max) px  ─┼─► screen_rect_to_target ─► NineSliceSource
//!   scrim  (0, screen) px ─┘        (Y flip)              ::expand      ─► Sprite
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
//!   `art.rs` still loads it, each game still writes the code that lays it out —
//!   so the duplication that actually costs something is untouched, and two
//!   build scripts grow a `../../..` path out of their own package, which cargo
//!   does not track for rebuilds the way it tracks a package's own files. It
//!   also gives `crcbl-vk`'s end-to-end suite nothing: that crate cannot see
//!   `apps/`, so the golden image would have to be taken of a *replica* of the
//!   menu rather than of the menu.
//! * **This crate.** Picked. It already owns [`NineSliceSource`],
//!   [`ButtonSkin`] and [`screen_rect_to_target`] — the three things a skinned
//!   menu is made of — and already depends on `crcbl-sprite` (which bakes and
//!   loads) and on `crcbl-ui` (which lays out). Every sample already depends on
//!   it. And `crcbl-vk`'s suite dev-depends on it, which is what lets the golden
//!   image be taken of the **real** art through the **real** expansion rather
//!   than of a lookalike built in the test.
//!
//! What it costs: `crcbl-render` grows a `build.rs` and a `png` decoder it did
//! not have. The decoder is the same one every sample already links, so no
//! binary gained a dependency; the build script is a third copy of a script
//! `docs/backlog.md` already wants folded into `crcbl-sprite`.
//!
//! # One sheet, one batch
//!
//! All five frames — the window, the three button states and the scrim — are in
//! `assets/menu.crpix`, so a whole menu is **one bind and one draw**. That is
//! [`ButtonSkin`]'s own argument for keeping its three frames together, applied
//! one level up.
//!
//! It is also currently load-bearing for correctness, which it should not have
//! to be: `sprite.slang` reads `sprites[InstanceIndex - BaseInstance]`, so
//! [`SpriteRenderer`]'s second batch in a frame re-reads the *first* batch's
//! instances. A three-sheet menu draws its panel with the buttons' art. See
//! `docs/backlog.md` — the samples' own art has four sheets each and is affected
//! today; this slice found it and did not fix it.
//!
//! # Screen space, and why the menu has its own pass
//!
//! A game's sprite pass draws in **its** world: `apps/breakout` at ten texels to
//! the world unit, `apps/flappy` at twenty, both through a camera whose extent
//! changes with the aspect ratio. A menu is not in that space — it is pinned to
//! the framebuffer, it is measured in pixels, and it must be the same size on
//! screen whatever the game's camera is doing.
//!
//! [`SpriteRenderer::begin_frame`] takes **one** view-projection for the whole
//! frame, so the two cannot share a pass. [`MenuRenderer`] therefore owns a
//! second [`SpriteRenderer`] with a screen-space orthographic camera — one world
//! unit per device pixel, centred on the framebuffer — which is exactly the
//! space [`screen_rect_to_target`] converts into. That is one extra pipeline per
//! sample and no extra draw on a frame with no menu:
//! [`SpriteRenderer::add_pass`] declares nothing when there are no sprites.
//!
//! **Declaration order is execution order.** The menu pass must be added after
//! the game's sprite pass and *before* the UI pass, or the scrim dims the menu's
//! own frame and the panel paints over its own labels.

use crcbl_hal::{Device, HalError, QueueHandle};
use crcbl_sprite::load::{Loaded, load};
use crcbl_ui::SkinInsets;
use crcbl_ui::menu::{Menu, MenuLayout};
use glam::{Mat4, Vec3};

use crate::button_skin::{ButtonSkin, screen_rect_to_target};
use crate::camera::{Camera, Projection};
use crate::nine_slice::NineSliceSource;
use crate::sprite_pass::{SheetDesc, SheetId, Sprite, SpriteRenderer};

// `build.rs` writes this: one `*_PNG` and one `*_JSON` per `assets/*.crpix`.
include!(concat!(env!("OUT_DIR"), "/menu_art.rs"));

/// "The sheet as authored": no tint on the frame or the buttons.
const UNTINTED: [f32; 4] = [1.0; 4];

/// How far the menu's camera sits from the plane its sprites live on.
///
/// Any value inside the near/far pair works — the sprites are flat at z = 0 —
/// and it is written down rather than defaulted because
/// `tests::the_camera_puts_one_world_unit_on_one_device_pixel` measures the
/// matrix this builds.
const MENU_CAMERA_Z: f32 = 1.0;

// ---------------------------------------------------------------------------
// The art
// ---------------------------------------------------------------------------

/// The one sheet a menu is drawn from, and the frames cut out of it.
///
/// One sheet and five frames — see the module docs. Every frame shares the
/// sheet's single `nine`, which is why the panel and the buttons are both
/// `4 4 4 4`: a [`Sheet`](crcbl_sprite::Sheet) carries one set of insets for all
/// its frames, and agreeing on them is what lets
/// [`NineSliceSource::from_sheet`] and [`ButtonSkin::from_sheet`] be used as
/// they are rather than hand-built from sub-rectangles.
#[derive(Clone, Copy, Debug)]
pub struct MenuArt {
    sheet: SheetId,
    panel: NineSliceSource,
    scrim_uv: [f32; 4],
    skin: ButtonSkin,
}

/// Which frame of `assets/menu.crpix` is which.
///
/// Named rather than spelled as literals at the two places that index the sheet,
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

impl MenuArt {
    /// Uploads the sheet and resolves the five frames this module names.
    ///
    /// One blocking staging upload — start-up work, never something a frame
    /// does.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the upload or the bind group was refused. Nothing here
    /// fails on the *art*: it was parsed, validated and baked by `build.rs`, so
    /// a sheet that will not load is a bug in this repository rather than a
    /// runtime condition.
    ///
    /// # Panics
    ///
    /// If the baked sheet cannot be read back, or does not declare the frames
    /// and insets this module names.
    pub fn register(device: &dyn Device, sprites: &mut SpriteRenderer) -> Result<Self, HalError> {
        let art = baked("menu", MENU_PNG, MENU_JSON);
        let sheet = register(device, sprites, "menu", &art)?;

        Ok(Self {
            sheet,
            panel: NineSliceSource::from_sheet(&art.sheet, PANEL_FRAME)
                .expect("menu.crpix declares a nine-slice over five frames"),
            scrim_uv: art
                .sheet
                .uv(SCRIM_FRAME)
                .expect("menu.crpix has a scrim frame"),
            skin: ButtonSkin::from_sheet(
                sheet,
                &art.sheet,
                IDLE_FRAME,
                HOVERED_FRAME,
                PRESSED_FRAME,
            )
            .expect("menu.crpix declares a frame per button state"),
        })
    }

    /// The window frame's fixed corners, in **texels**.
    ///
    /// The texel source figure [`crcbl_ui::menu::PANEL_INSETS`] mirrors — the
    /// comparison `tests::the_shipped_art_has_the_insets_the_layout_assumes`
    /// runs to keep the layout's corners and the art's in step.
    #[must_use]
    pub fn panel_insets(&self) -> SkinInsets {
        let nine = self.panel.insets();
        SkinInsets::new(
            nine.left as f32,
            nine.right as f32,
            nine.top as f32,
            nine.bottom as f32,
        )
    }

    /// A button's fixed corners, in **texels**.
    #[must_use]
    pub fn button_insets(&self) -> SkinInsets {
        self.skin.insets()
    }

    /// The button skin, for a caller that wants to draw one on its own.
    #[must_use]
    pub const fn skin(&self) -> &ButtonSkin {
        &self.skin
    }

    /// Appends the sprites that draw `menu` at `layout`, back to front.
    ///
    /// **The order is the compositing order** — [`Sprite`]'s own rule — so it is
    /// the scrim, then the window frame, then one button at a time in the order
    /// the layout placed them. A caller that reordered them would draw the panel
    /// under the game it is supposed to be covering.
    ///
    /// Every rectangle goes through [`screen_rect_to_target`] against a viewport
    /// of [`MenuLayout::screen`] and a camera centred on the origin, so a target
    /// is measured in the same **device pixels** the camera's world is — see
    /// [`menu_camera`]. The style's scale enters through the art instead: the
    /// panel and the button skin are built per call at `1 / scale` texels per
    /// unit, which is what turns the layout's pixel corners back into the texel
    /// bands the art refuses to stretch. A caller building its own pass owes the
    /// same camera and the same scaled sources.
    pub fn extend(&self, menu: &Menu, layout: &MenuLayout, out: &mut Vec<Sprite>) {
        let screen = layout.screen();
        let viewport = (screen.x as u32, screen.y as u32);
        let scale = layout.style().scale.max(1.0);
        let target = |min, max| screen_rect_to_target(min, max, viewport, [0.0, 0.0]);

        let (scrim_min, scrim_max) = layout.scrim();
        // Unrotated: a menu is screen furniture; nothing in it turns.
        out.push(
            Sprite::new(self.sheet, target(scrim_min, scrim_max), self.scrim_uv)
                .with_tint(layout.style().scrim_color),
        );

        let panel = self.panel.with_texels_per_unit(1.0 / scale);
        let (panel_min, panel_max) = layout.panel();
        out.extend(
            panel
                .expand(target(panel_min, panel_max))
                .sprites(self.sheet, UNTINTED)
                // `sprites` borrows the `NineQuads` it was called on, which is a
                // temporary here — collected rather than lent onwards.
                .collect::<Vec<_>>(),
        );

        let skin = self.skin.with_texels_per_unit(1.0 / scale);
        for (index, placed) in layout.items().iter().enumerate() {
            out.extend(
                skin.quads(menu.state(index), target(placed.min, placed.max))
                    .sprites(skin.sheet, UNTINTED)
                    .collect::<Vec<_>>(),
            );
        }
    }
}

/// The camera the menu is drawn with: **one world unit per device pixel**,
/// centred on the framebuffer, looking down −Z.
///
/// # Why one unit per pixel
///
/// The menu's world is the framebuffer's own. Every rectangle
/// [`MenuArt::extend`] lays out is measured in device pixels, so the camera
/// that projects them needs no scale of its own — the art's texels enter on
/// the [`NineSliceSource`]s instead, which [`MenuArt::extend`] builds at
/// `1 / scale` texels per unit. That is what keeps a four-texel corner at
/// `4 * scale` device pixels whatever scale the menu is drawn at: the layout
/// asks for pixel corners and the scaled source turns the fixed texel bands
/// back into those same pixels.
///
/// The width follows for free: [`Projection::Orthographic`] derives it from the
/// aspect ratio, which is `width / height`, so a half-height of `height / 2`
/// is one pixel per unit on both axes at **every** aspect ratio.
#[must_use]
pub fn menu_camera(extent: (u32, u32)) -> Camera {
    Camera {
        eye: Vec3::new(0.0, 0.0, MENU_CAMERA_Z),
        target: Vec3::ZERO,
        up: Vec3::Y,
        projection: Projection::Orthographic {
            half_height: extent.1.max(1) as f32 * 0.5,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// The view-projection [`MenuRenderer`] hands the pass for an `extent`-sized
/// framebuffer, at one world unit per device pixel.
#[must_use]
pub fn menu_view_projection(extent: (u32, u32)) -> Mat4 {
    let aspect = extent.0.max(1) as f32 / extent.1.max(1) as f32;
    menu_camera(extent).view_projection(aspect)
}

// ---------------------------------------------------------------------------
// The renderer
// ---------------------------------------------------------------------------

/// Draws a menu: the sheets, the pass, and the screen-space camera that joins
/// them.
///
/// One per sample, built once and kept. See the module docs for why it owns a
/// [`SpriteRenderer`] of its own rather than borrowing the game's.
#[derive(Debug)]
pub struct MenuRenderer {
    sprites: SpriteRenderer,
    art: MenuArt,
    /// This frame's sprites, refilled rather than reallocated.
    frame: Vec<Sprite>,
}

impl MenuRenderer {
    /// Builds the pass and uploads the shared art.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the device refused the sprite pipeline or a sheet upload.
    /// A failure leaves nothing behind: the renderer is destroyed before the
    /// error is returned.
    pub fn new(
        device: &dyn Device,
        queue: QueueHandle,
        format: crcbl_hal::Format,
    ) -> Result<Self, HalError> {
        let mut sprites = SpriteRenderer::new(device, queue, format)?;
        let art = match MenuArt::register(device, &mut sprites) {
            Ok(art) => art,
            Err(error) => {
                sprites.destroy(device);
                return Err(error);
            }
        };
        Ok(Self {
            sprites,
            art,
            frame: Vec::new(),
        })
    }

    /// The art, for a caller that wants the insets or the skin.
    #[must_use]
    pub const fn art(&self) -> &MenuArt {
        &self.art
    }

    /// The sprites this renderer will draw, in order.
    ///
    /// So a test can assert what a paused frame submitted with no device that
    /// can present.
    #[must_use]
    pub fn frame_sprites(&self) -> &[Sprite] {
        &self.frame
    }

    /// Takes this frame's menu, on the CPU only.
    ///
    /// `None` on a frame with no menu on it, which submits nothing and makes
    /// [`add_pass`](Self::add_pass) declare no pass at all.
    ///
    /// **Split from [`begin_frame`](Self::begin_frame) because the two have
    /// different callers.** The menu and its layout belong to the game loop and
    /// are borrowed from it; the upload belongs inside the frame, after the
    /// swapchain is acquired and its real extent is known. Holding the borrow
    /// across that would make this type carry the loop's lifetime for no reason.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.frame.clear();
        if let Some((menu, layout)) = menu {
            self.art.extend(menu, layout, &mut self.frame);
        }
    }

    /// Uploads whatever [`set_menu`](Self::set_menu) last took.
    ///
    /// # Errors
    ///
    /// [`HalError`] from the instance or constant buffers.
    pub fn begin_frame(&mut self, device: &dyn Device, extent: (u32, u32)) -> Result<(), HalError> {
        self.sprites
            .begin_frame(device, &self.frame, menu_view_projection(extent), extent)
    }

    /// The most passes [`add_pass`](Self::add_pass) adds to a frame.
    ///
    /// The sprite pass's own number, because the menu *is* a sprite pass: a
    /// frame with no menu on it declares nothing at all.
    pub const MAX_PASSES: u32 = crate::sprite_pass::SpriteRenderer::MAX_PASSES;

    /// Adds the menu pass to `graph`, drawing on top of `target`.
    ///
    /// **After the game's passes and before the UI pass** — see the module docs.
    pub fn add_pass<'a>(
        &'a self,
        graph: &mut crate::graph::RenderGraph<'a>,
        target: crate::graph::ImageId,
    ) {
        self.sprites.add_pass(graph, target);
    }

    /// Releases the pass and every sheet. The device must be idle.
    pub fn destroy(self, device: &dyn Device) {
        self.sprites.destroy(device);
    }
}

// ---------------------------------------------------------------------------
// Start-up helpers
// ---------------------------------------------------------------------------

/// Decodes one baked sheet.
fn baked(name: &str, png: &[u8], json: Option<&str>) -> Loaded {
    load(png, json, ART_TICK_HZ)
        .unwrap_or_else(|error| panic!("the baked {name} sheet did not load: {error}"))
}

fn register(
    device: &dyn Device,
    sprites: &mut SpriteRenderer,
    label: &str,
    loaded: &Loaded,
) -> Result<SheetId, HalError> {
    sprites.register_sheet(
        device,
        &SheetDesc {
            label,
            width: loaded.image.width,
            height: loaded.image.height,
            sample: loaded.sheet.sample,
            pixels: &loaded.image.pixels,
        },
    )
}

/// The [`Sheet`](crcbl_sprite::Sheet) a baked pair describes, for the tests
/// below.
#[cfg(test)]
fn sheet_of(loaded: &Loaded) -> &crcbl_sprite::Sheet {
    &loaded.sheet
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::null::NullInstance;
    use crcbl_hal::{DeviceDesc, Format, Instance, QueueKind};
    use crcbl_sprite::NineSlice;
    use crcbl_ui::ButtonState;
    use crcbl_ui::menu::{MenuItem, MenuStyle};
    use crcbl_ui::text::FontAtlas;
    use glam::Vec2;

    /// The five extents every "it is on screen" test in this repository uses.
    const EXTENTS: [(u32, u32); 5] = [
        (960, 720),
        (800, 600),
        (1920, 1080),
        (1440, 400),
        (600, 900),
    ];

    fn pause_menu() -> Menu {
        Menu::new(
            "PAUSED",
            vec![
                MenuItem::new(1, "RESUME", "ESC"),
                MenuItem::new(2, "FULLSCREEN", "F11"),
                MenuItem::new(3, "DEBUG OVERLAY", "F3"),
            ],
        )
    }

    /// Runs `body` against the art registered on the null backend.
    ///
    /// A real [`SpriteRenderer`], because [`SheetId`] can only come from one.
    fn with_art(body: impl FnOnce(&MenuArt)) {
        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut sprites = SpriteRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("the null backend accepts the sprite pipeline");
        let art = MenuArt::register(device.as_ref(), &mut sprites).expect("the sheets upload");
        body(&art);
        sprites.destroy(device.as_ref());
    }

    /// Frame `index` of a sheet, as its own block of RGBA, cut out of the strip.
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
            .chunks_exact(4)
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
                .chunks_exact(4)
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
        with_art(|art| {
            assert_eq!(
                art.panel_insets(),
                crcbl_ui::menu::PANEL_INSETS,
                "menu.crpix and crcbl_ui::menu::PANEL_INSETS disagree",
            );
            assert_eq!(
                art.button_insets(),
                crcbl_ui::menu::BUTTON_INSETS,
                "menu.crpix and crcbl_ui::menu::BUTTON_INSETS disagree",
            );
            assert!(
                art.skin().insets_agree(),
                "the three button frames must share their insets",
            );
        });
    }

    // -----------------------------------------------------------------------
    // The camera
    // -----------------------------------------------------------------------

    /// **One world unit of the menu is one device pixel**, at every aspect
    /// ratio — which is what makes a nine-slice's four-texel corner, at
    /// `1 / scale` texels per unit on the source, come out four *scaled*
    /// pixels rather than four pixels, and what makes every pixel claim in the
    /// golden suite arithmetic rather than a number read off a picture.
    #[test]
    fn the_camera_puts_one_world_unit_on_one_device_pixel() {
        for extent in EXTENTS {
            let (width, height) = (extent.0 as f32, extent.1 as f32);
            let view_projection = menu_view_projection(extent);
            for world in [
                [0.0, 0.0],
                [-width / 2.0, height / 2.0],
                [width / 2.0, -height / 2.0],
                [17.0, -23.0],
            ] {
                let clip = view_projection * glam::Vec4::new(world[0], world[1], 0.0, 1.0);
                let ndc = clip.truncate() / clip.w;
                // +Y is up in NDC and the backends submit a flipped viewport,
                // so the framebuffer row is the negated half.
                let pixel = Vec2::new(
                    0.5f32.mul_add(ndc.x, 0.5) * width,
                    0.5f32.mul_add(-ndc.y, 0.5) * height,
                );
                let expected = Vec2::new(world[0] + width / 2.0, -world[1] + height / 2.0);
                assert!(
                    (pixel - expected).abs().max_element() < 1e-2,
                    "{extent:?}: world {world:?} lands at {pixel:?}, not \
                     {expected:?}",
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // What a frame submits
    // -----------------------------------------------------------------------

    /// The sprites are the scrim, then the frame, then the buttons — in that
    /// order, on the sheets they belong to.
    #[test]
    fn a_menu_submits_the_scrim_then_the_frame_then_its_buttons() {
        with_art(|art| {
            let menu = pause_menu();
            let atlas = FontAtlas::built_in();
            let layout = menu.layout((960, 720), &atlas);
            let mut sprites = Vec::new();
            art.extend(&menu, &layout, &mut sprites);

            // One scrim, nine panel quads (every band non-empty at this size),
            // nine per button.
            assert_eq!(sprites.len(), 1 + 9 + 9 * 3, "{}", sprites.len());
            assert_eq!(sprites[0].tint, layout.style().scrim_color);
            // **One sheet, so one batch.** The sprite pass starts a new batch
            // every time the sheet changes between consecutive sprites, and a
            // menu that grew a second sheet would cost a bind and a draw per
            // switch — and, until the finding in `docs/backlog.md` is fixed,
            // would draw the panel with the buttons' art.
            assert!(
                sprites.iter().all(|s| s.sheet == art.sheet),
                "the menu is drawn from more than one sheet",
            );
            // The scrim, the frame and the buttons are still three *different
            // pictures* out of that one sheet — cut apart by UV, which is what
            // the sheet ids used to say.
            assert_ne!(sprites[0].uv, sprites[1].uv);
            assert_ne!(sprites[1].uv, sprites[10].uv);

            // The scrim covers the whole framebuffer, which is what makes it a
            // scrim rather than a rectangle behind the panel. In pixels, because
            // that is the space the camera projects — see `menu_camera`.
            assert_eq!(sprites[0].rect, [-480.0, -360.0, 960.0, 720.0],);
        });
    }

    /// **The frame is centred on the framebuffer**, at every aspect ratio,
    /// measured on the quads the pass will actually draw rather than on the
    /// layout that produced them.
    ///
    /// The camera puts the framebuffer's centre at the world origin, so "centred"
    /// is "the union of the nine quads is symmetric about zero". Everything here
    /// is in **device pixels** — the space the camera projects — so the
    /// framebuffer's half-extent is its own pixels, and the tolerance is half a
    /// device pixel, for the whole-pixel rounding `crcbl_ui::Menu::layout_with`
    /// applies.
    #[test]
    fn the_window_frame_is_centred_on_the_framebuffer_at_every_aspect_ratio() {
        with_art(|art| {
            let menu = pause_menu();
            let atlas = FontAtlas::built_in();
            for extent in EXTENTS {
                let layout = menu.layout(extent, &atlas);
                let mut sprites = Vec::new();
                art.extend(&menu, &layout, &mut sprites);
                let frame = &sprites[1..10];

                let left = frame
                    .iter()
                    .fold(f32::INFINITY, |acc, s| acc.min(s.rect[0]));
                let right = frame
                    .iter()
                    .fold(f32::NEG_INFINITY, |acc, s| acc.max(s.rect[0] + s.rect[2]));
                let bottom = frame
                    .iter()
                    .fold(f32::INFINITY, |acc, s| acc.min(s.rect[1]));
                let top = frame
                    .iter()
                    .fold(f32::NEG_INFINITY, |acc, s| acc.max(s.rect[1] + s.rect[3]));

                let tolerance = 0.5;
                assert!(
                    ((left + right) / 2.0).abs() <= tolerance,
                    "{extent:?}: the frame runs {left}..{right}, which is not \
                     centred on the origin",
                );
                assert!(
                    ((bottom + top) / 2.0).abs() <= tolerance,
                    "{extent:?}: the frame runs {bottom}..{top} vertically",
                );
                // And it is really on the screen — a "centred" frame twice the
                // size of the framebuffer is also symmetric about zero.
                let half = Vec2::new(extent.0 as f32, extent.1 as f32) * 0.5;
                assert!(
                    left >= -half.x && right <= half.x,
                    "{extent:?}: the frame is wider than the framebuffer",
                );
                assert!(
                    bottom >= -half.y && top <= half.y,
                    "{extent:?}: the frame is taller than the framebuffer",
                );
            }
        });
    }

    /// **The window frame's corners keep their size as the menu grows.** The
    /// nine-slice property, asserted at the level a player sees it: one short
    /// menu and one long one, and the four corner quads come back the same size
    /// and sampling the same texels, while the edges take all of the difference.
    ///
    /// Both halves matter. Corners that keep their UVs and grow are smudged art;
    /// corners that keep their size and slide their UVs sample the wrong texels.
    /// And without the edge half, a renderer that drew nothing but the corners
    /// would pass.
    #[test]
    fn the_frames_corners_do_not_grow_with_the_menu() {
        with_art(|art| {
            let atlas = FontAtlas::built_in();
            let style = MenuStyle::pixel_art(3);
            let small = Menu::new("GO", vec![MenuItem::new(1, "OK", "")]);
            let large = Menu::new(
                "A MUCH LONGER TITLE",
                (0..5)
                    .map(|i| MenuItem::new(i, "A VERY LONG MENU ITEM LABEL", "SHIFT+F11"))
                    .collect(),
            );

            let quads = |menu: &Menu| {
                let layout = menu.layout_with((1600, 1200), &atlas, &style);
                let mut sprites = Vec::new();
                art.extend(menu, &layout, &mut sprites);
                sprites[1..10].to_vec()
            };
            let (a, b) = (quads(&small), quads(&large));

            // The two really are different sizes, or the rest proves nothing.
            let width = |q: &[Sprite]| q.iter().map(|s| s.rect[2]).sum::<f32>() / 3.0;
            assert!(
                width(&b) > width(&a) * 1.5,
                "the two menus are {} and {} wide",
                width(&a),
                width(&b),
            );

            // In image order: 0, 2, 6 and 8 are the corners; 1, 3, 5, 7 the
            // edges.
            for corner in [0usize, 2, 6, 8] {
                assert_eq!(
                    [a[corner].rect[2], a[corner].rect[3]],
                    [b[corner].rect[2], b[corner].rect[3]],
                    "corner {corner} changed size with the menu",
                );
                assert_eq!(
                    a[corner].uv, b[corner].uv,
                    "corner {corner} slid its UVs, so it is sampling different \
                     texels at the two sizes",
                );
                // And it is the art's own inset turned into pixels — the texel
                // figure at `1 / scale` texels per unit, which is the same
                // number `pixel_art` pre-multiplies the layout's corners to.
                assert_eq!(
                    a[corner].rect[2],
                    crcbl_ui::menu::PANEL_INSETS.left * style.scale
                );
            }
            // The edges took the difference — the half without which "the
            // corners are identical" is satisfied by drawing only corners.
            assert!(
                b[1].rect[2] > a[1].rect[2] && b[3].rect[3] >= a[3].rect[3],
                "the stretched bands did not absorb the growth",
            );
        });
    }

    /// **A button's state changes the UVs it samples, and not the rectangle it
    /// covers.** Asserted as UVs rather than as a bool, because a `ButtonState`
    /// that never reached the art is a menu whose buttons never light up and
    /// whose every layout test still passes.
    #[test]
    fn a_state_change_moves_the_uvs_and_leaves_the_rectangle_alone() {
        with_art(|art| {
            let atlas = FontAtlas::built_in();
            let mut menu = pause_menu();
            let layout = menu.layout((960, 720), &atlas);

            let button_quads = |menu: &Menu, index: usize| {
                let mut sprites = Vec::new();
                art.extend(menu, &layout, &mut sprites);
                sprites[10 + index * 9..10 + (index + 1) * 9].to_vec()
            };

            // Item 0 is selected, so it draws `Hovered` and the others `Idle`.
            let idle = button_quads(&menu, 1);
            let hovered = button_quads(&menu, 0);
            menu.press(true);
            let pressed = button_quads(&menu, 0);

            assert_eq!(menu.state(0), ButtonState::Pressed);
            assert_eq!(menu.state(1), ButtonState::Idle);

            for quad in 0..9 {
                assert_ne!(
                    hovered[quad].uv, pressed[quad].uv,
                    "quad {quad} samples the same texels hovered and pressed",
                );
                assert_eq!(
                    hovered[quad].rect, pressed[quad].rect,
                    "quad {quad} moved when the button was pressed",
                );
            }
            // The idle button is a different rectangle (it is a different item)
            // but the same *shape*, and its UVs are the third frame's.
            for quad in 0..9 {
                assert_ne!(idle[quad].uv, hovered[quad].uv);
                assert_ne!(idle[quad].uv, pressed[quad].uv);
                assert_eq!(
                    [idle[quad].rect[2], idle[quad].rect[3]],
                    [hovered[quad].rect[2], hovered[quad].rect[3]],
                    "two buttons of one menu are not the same size",
                );
            }
        });
    }

    /// A frame with no menu on it submits nothing at all — which is what makes
    /// the pass free rather than cheap.
    #[test]
    fn a_frame_with_no_menu_submits_no_sprites() {
        with_art(|art| {
            let menu = Menu::new("EMPTY", Vec::new());
            let atlas = FontAtlas::built_in();
            let layout = menu.layout((960, 720), &atlas);
            let mut sprites = Vec::new();
            art.extend(&menu, &layout, &mut sprites);
            // The scrim and the frame, and no buttons: an empty menu is still a
            // panel, and a caller that wants nothing drawn passes `None` to
            // `MenuRenderer::begin_frame` instead.
            assert_eq!(sprites.len(), 1 + 9);
        });
    }
}
