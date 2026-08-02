//! Skinned buttons: a nine-sliced frame per interaction state, drawn as sprites.
//!
//! ```text
//!    idle              hovered           pressed
//!   ┌──┬────────┬──┐  ┌──┬────────┬──┐  ┌──┬────────┬──┐
//!   │  │        │  │  │  │        │  │  │  │        │  │
//!   ├──┤  Play  ├──┤  ├──┤  Play  ├──┤  ├──┤  Play  ├──┤
//!   │  │        │  │  │  │        │  │  │  │        │  │
//!   └──┴────────┴──┘  └──┴────────┴──┘  └──┴────────┴──┘
//!    three frames of one sheet — pressing swaps art, it does not tint
//! ```
//!
//! # Why a button's skin is a sprite and not a UI-pass primitive
//!
//! `docs/backlog.md` listed this item as blocked on "the UI pass being able to
//! sample a second texture". **That blocker is no longer the right reading of
//! the tree, and taking it literally would be the expensive way round.**
//!
//! The UI pass has exactly one texture and it is a *glyph coverage mask*:
//! [`UiRenderer`](crate::UiRenderer) uploads the built-in atlas as
//! `Format::R8Unorm`, and `shaders/ui.slang` samples that single channel into
//! **alpha only** — every fragment's RGB comes from the vertex colour, and a UV
//! of `(0, 0)` is the sentinel for "untextured". A button skin is RGBA colour
//! art. Routing it through the UI pass would mean a second bound image in a
//! second format, a per-quad branch selecting between two samplers, a
//! UV-carrying draw command that [`DrawList`](crcbl_ui::DrawList) does not have,
//! and an RGB path in a shader that would need adding to both of its tier
//! permutations by hand.
//!
//! All of which duplicates a pass that now exists. [`SpriteRenderer`] is an
//! instanced `Rgba8UnormSrgb` pass with alpha blending and both sample modes,
//! and [`NineSliceSource::expand`] already turns insets into the nine quads with
//! the corners left alone. A skinned button is nine sprites. There is nothing to
//! build in the UI pass at all.
//!
//! So the blocker was **wrong**, and it was wrong because it was written before
//! the sprite pass existed — when the UI pass was the only thing that could draw
//! a 2D quad, teaching it a second texture really was the only route. Slice 6
//! removed the need rather than satisfying it.
//!
//! # What that costs, and who pays it
//!
//! A skinned button is drawn by **two passes**, and the caller owns the join:
//!
//! ```text
//!   forward / scene  ──►  sprite pass (the skin)  ──►  UI pass (the label)
//! ```
//!
//! [`RenderGraph`](crate::RenderGraph) runs passes in **declaration order** —
//! there is no topological sort, and `SpriteRenderer::add_pass` and
//! `UiRenderer::add_pass` both load rather than clear. So
//! `SpriteRenderer::add_pass` must be called **before** `UiRenderer::add_pass`
//! or the skin paints over its own text. Nothing enforces it but the order of
//! two lines; `apps/breakout/src/gpu.rs` and `apps/flappy/src/gpu.rs` today add
//! the forward passes and then the UI pass, and the sprite pass goes between
//! them.
//!
//! The other half of the join is layout. [`crcbl_ui::Button`] cannot name a
//! sheet or a [`NineSlice`](crcbl_sprite::NineSlice) — `crcbl-render` depends
//! on `crcbl-ui`, so the
//! reverse is a cycle — and it does not need to. It carries the four floats it
//! needs to lay out *around* the art, as [`SkinInsets`], and
//! [`ButtonSkin::insets`] reads them straight off the art so the two cannot
//! drift.
//!
//! [`SpriteRenderer`]: crate::SpriteRenderer

use glam::Vec2;

use crcbl_sprite::Sheet;
use crcbl_ui::{ButtonState, SkinInsets};

use crate::nine_slice::{NineQuads, NineSliceSource};
use crate::sprite_pass::SheetId;

/// The art a skinned button draws from: one nine-slice frame per
/// [`ButtonState`], all in one sheet.
///
/// Three frames rather than one frame and three tints, because that is what the
/// feature is for. A tint can only darken or lighten uniformly; separate frames
/// let a pressed button have a sunken bevel, a different highlight, or a shadow
/// that moved — none of which is a multiply.
///
/// # Drawing one
///
/// ```ignore
/// let target = screen_rect_to_target(min, max, viewport, [0.0, 0.0]);
/// stack.extend(ui_layer, skin.quads(state, target).sprites(skin.sheet, [1.0; 4]));
/// ```
///
/// [`ButtonSkin::quads`] returns by value and allocates nothing;
/// [`NineQuads::sprites`] borrows it for the length of the statement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ButtonSkin {
    /// The sheet all three frames live in.
    ///
    /// One sheet and not three, so the whole button is a single
    /// [`Sprite`](crate::Sprite) batch: the sprite pass starts a new batch every
    /// time the sheet changes between consecutive sprites, so a button split
    /// across sheets would cost a bind and a draw per state change.
    pub sheet: SheetId,
    /// The frame drawn at rest.
    pub idle: NineSliceSource,
    /// The frame drawn under the cursor.
    pub hovered: NineSliceSource,
    /// The frame drawn while the button holds the press.
    pub pressed: NineSliceSource,
}

impl ButtonSkin {
    /// Three frames of one [`Sheet`], by index.
    ///
    /// `None` when the sheet declares no nine-slice, has no size, or is missing
    /// any of the three frames — [`NineSliceSource::from_sheet`]'s conditions,
    /// checked for each. A skin that silently fell back to the idle frame for a
    /// missing `pressed` would be a button that looks broken only while it is
    /// being clicked, which is the hardest moment to see it.
    #[must_use]
    pub fn from_sheet(
        sheet: SheetId,
        art: &Sheet,
        idle: usize,
        hovered: usize,
        pressed: usize,
    ) -> Option<Self> {
        Some(Self {
            sheet,
            idle: NineSliceSource::from_sheet(art, idle)?,
            hovered: NineSliceSource::from_sheet(art, hovered)?,
            pressed: NineSliceSource::from_sheet(art, pressed)?,
        })
    }

    /// One frame for every state — a skin that does not react.
    ///
    /// Useful for a decorative panel, and for a test that wants to vary size
    /// without varying art.
    #[must_use]
    pub const fn uniform(sheet: SheetId, frame: NineSliceSource) -> Self {
        Self {
            sheet,
            idle: frame,
            hovered: frame,
            pressed: frame,
        }
    }

    /// The frame `state` draws.
    #[must_use]
    pub const fn source(&self, state: ButtonState) -> &NineSliceSource {
        match state {
            ButtonState::Idle => &self.idle,
            ButtonState::Hovered => &self.hovered,
            ButtonState::Pressed => &self.pressed,
        }
    }

    /// The insets to hand [`crcbl_ui::Button::with_skin`], in pixels.
    ///
    /// Read off the **idle** frame, and off its trimmed insets rather than its
    /// declared ones, so the layout agrees with the geometry that will actually
    /// be emitted.
    ///
    /// Idle specifically, and not "whichever state is current": a button whose
    /// minimum size changed with the cursor would resize as the mouse crossed it,
    /// and in a layout that packs buttons it would shove its neighbours around.
    /// Frames of one skin are expected to share their insets —
    /// [`ButtonSkin::from_sheet`] cannot produce a skin where they do not,
    /// because a [`Sheet`] carries one `nine` for all its frames — and
    /// [`ButtonSkin::insets_agree`] is there for a skin built by hand.
    ///
    /// One world unit per texel, which is the ratio
    /// [`NineSliceSource::expand`] draws the fixed bands at.
    #[must_use]
    pub fn insets(&self) -> SkinInsets {
        let nine = self.idle.insets();
        SkinInsets::new(
            nine.left as f32,
            nine.right as f32,
            nine.top as f32,
            nine.bottom as f32,
        )
    }

    /// Whether all three frames have the same trimmed insets.
    ///
    /// [`ButtonSkin::insets`] describes only the idle frame, so a hand-built skin
    /// that disagrees would lay out to one frame's corners and draw another's.
    #[must_use]
    pub fn insets_agree(&self) -> bool {
        let idle = self.idle.insets();
        self.hovered.insets() == idle && self.pressed.insets() == idle
    }

    /// The quads that draw `state`'s frame stretched to `target`.
    ///
    /// `target` is `[x, y, w, h]` in **world units, Y up**, minimum corner first
    /// — [`Sprite::rect`](crate::Sprite)'s layout, not a screen rect. Use
    /// [`screen_rect_to_target`] to get one from a [`crcbl_ui::Button`].
    #[must_use]
    pub fn quads(&self, state: ButtonState, target: [f32; 4]) -> NineQuads {
        self.source(state).expand(target)
    }
}

/// A `crcbl-ui` screen rectangle as a sprite-pass target.
///
/// This is the one place the two coordinate conventions in this engine meet, and
/// getting it wrong is silent: a button drawn with an un-flipped Y is still a
/// button, just mirrored about the middle of the screen, and at the centre of the
/// viewport it is not wrong at all.
///
/// * [`crcbl_ui`] lays out in **screen pixels, Y down**, origin at the
///   framebuffer's top-left, `min` the visually-upper corner. That is what
///   `shaders/ui.slang` implements and what every widget measures in.
/// * [`Sprite::rect`](crate::Sprite) is **world units, Y up**, minimum corner
///   first.
///
/// `viewport` is the framebuffer's size in pixels and `centre` the world point
/// the middle of the viewport sits at, which is what makes this work for any
/// screen-pinned orthographic camera at one world unit per pixel: `[0.0, 0.0]`
/// for a camera centred on the origin, `[w / 2.0, h / 2.0]` for one whose origin
/// is the bottom-left. For a camera that moves, pass its position — this is the
/// `Parallax::CAMERA` case, where the layer is pinned to the screen and the
/// world point under it is wherever the camera is looking.
///
/// The returned width and height are the screen rect's own, so a caller that
/// hands in an inverted rect gets an inverted target, which
/// [`NineSliceSource::expand`] then draws as nothing.
#[must_use]
pub fn screen_rect_to_target(
    min: Vec2,
    max: Vec2,
    viewport: (u32, u32),
    centre: [f32; 2],
) -> [f32; 4] {
    let half = Vec2::new(viewport.0 as f32, viewport.1 as f32) * 0.5;
    [
        // X runs the same way in both conventions, so only the origin moves.
        centre[0] - half.x + min.x,
        // Y is flipped, so the rect's *lower* world edge comes from its
        // *larger* screen coordinate — `max.y`, not `min.y`.
        centre[1] + half.y - max.y,
        max.x - min.x,
        max.y - min.y,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_sprite::{Frame, NineSlice, Rect, SampleMode};
    use crcbl_ui::{Button, FontAtlas};

    /// A 64×64 sheet holding three 16×16 frames in its top row, side by side, so
    /// the three states have **different UVs and the same insets** — which is
    /// exactly the pair of properties the tests below separate.
    const FRAME_SIZE: u32 = 16;
    const SHEET_SIZE: u32 = 64;

    /// Insets different on all four sides, so a transposed or mirrored pair
    /// cannot compare equal by accident.
    const NINE: NineSlice = NineSlice::new(3, 5, 2, 4);

    fn art() -> Sheet {
        Sheet {
            width: SHEET_SIZE,
            height: SHEET_SIZE,
            frames: (0..3)
                .map(|index| Frame {
                    name: format!("state{index}"),
                    rect: Rect::new(index * FRAME_SIZE, 0, FRAME_SIZE, FRAME_SIZE),
                    hold: 1,
                })
                .collect(),
            clips: Vec::new(),
            nine: Some(NINE),
            sample: SampleMode::Pixel,
        }
    }

    fn skin() -> ButtonSkin {
        ButtonSkin::from_sheet(SheetId(0), &art(), 0, 1, 2).expect("three frames and a nine")
    }

    const STATES: [ButtonState; 3] = [
        ButtonState::Idle,
        ButtonState::Hovered,
        ButtonState::Pressed,
    ];

    /// The corner quads of a full nine, in image order. The edge quads — the
    /// ones that are *supposed* to change with size — are 1, 3, 5 and 7, and are
    /// named individually where they are checked because the horizontal pair and
    /// the vertical pair make opposite claims.
    const CORNERS: [usize; 4] = [0, 2, 6, 8];

    // -----------------------------------------------------------------------
    // The feature
    // -----------------------------------------------------------------------

    /// **The whole point of the slice.** One skin drawn at three very different
    /// widths must leave its four corners byte-identical in size *and* in UV,
    /// while the edges take all of the difference.
    ///
    /// Both halves are needed. Corners that keep their UVs and grow are smudged
    /// art; corners that keep their size and slide their UVs are sampling the
    /// wrong texels. And without the edge half, a renderer that drew *nothing but
    /// the four corners* at every size would pass.
    #[test]
    fn resizing_a_button_leaves_its_corner_quads_identical_in_size_and_uv() {
        let skin = skin();
        let widths = [40.0f32, 120.0, 600.0];

        let expansions: Vec<NineQuads> = widths
            .iter()
            .map(|width| skin.quads(ButtonState::Idle, [0.0, 0.0, *width, 48.0]))
            .collect();
        for (quads, width) in expansions.iter().zip(widths) {
            assert_eq!(quads.len(), 9, "at width {width} some band went empty");
        }

        let reference = &expansions[0];
        for (quads, width) in expansions.iter().zip(widths).skip(1) {
            for corner in CORNERS {
                assert_eq!(
                    quads[corner].rect[2], reference[corner].rect[2],
                    "corner {corner} changed width at {width} — the smudge this \
                     whole feature exists to prevent"
                );
                assert_eq!(
                    quads[corner].rect[3], reference[corner].rect[3],
                    "corner {corner} changed height at {width}"
                );
                assert_eq!(
                    quads[corner].uv, reference[corner].uv,
                    "corner {corner} sampled different texels at {width} — same \
                     size, wrong art"
                );
            }
        }

        // The corners really are the skin's insets, so "identical" is not
        // "identically wrong".
        let insets = skin.insets();
        assert_eq!(
            (reference[0].rect[2], reference[0].rect[3]),
            (insets.left, insets.top)
        );
        assert_eq!(
            (reference[8].rect[2], reference[8].rect[3]),
            (insets.right, insets.bottom)
        );

        // And the horizontal edges absorbed every pixel of the difference: the
        // top and bottom edges are the only quads whose width may change.
        for (quads, width) in expansions.iter().zip(widths).skip(1) {
            for edge in [1usize, 7] {
                assert!(
                    quads[edge].rect[2] > reference[edge].rect[2],
                    "edge {edge} did not stretch at width {width}"
                );
            }
            assert_eq!(
                quads[1].rect[2] - reference[1].rect[2],
                width - widths[0],
                "the top edge must take the whole difference in width"
            );
            // The vertical edges keep their width — only their height stretches,
            // and the height did not change here.
            for edge in [3usize, 5] {
                assert_eq!(quads[edge].rect[2], reference[edge].rect[2]);
                assert_eq!(quads[edge].rect[3], reference[edge].rect[3]);
            }
        }

        // Every quad still tiles the target exactly, at every width.
        for (quads, width) in expansions.iter().zip(widths) {
            let area: f32 = quads.iter().map(|q| q.rect[2] * q.rect[3]).sum();
            assert!(
                (area - width * 48.0).abs() < 1e-3,
                "at width {width} the quads cover {area}, not {}",
                width * 48.0
            );
        }
    }

    /// **Each state draws its own frame.** The UV rectangles must differ between
    /// states — a skin that recorded three states and sampled one is exactly what
    /// "pressing tints it" looks like from the outside.
    #[test]
    fn each_state_draws_its_own_frame() {
        let skin = skin();
        let target = [0.0, 0.0, 120.0, 48.0];
        let per_state: Vec<NineQuads> = STATES
            .iter()
            .map(|state| skin.quads(*state, target))
            .collect();

        for (a, state_a) in per_state.iter().zip(STATES) {
            for (b, state_b) in per_state.iter().zip(STATES) {
                if state_a == state_b {
                    continue;
                }
                // Every quad differs, not merely some quad somewhere: the frames
                // are disjoint rectangles of the sheet.
                for index in 0..a.len() {
                    assert_ne!(
                        a[index].uv, b[index].uv,
                        "quad {index} samples the same texels for {state_a:?} and \
                         {state_b:?} — the state swapped nothing"
                    );
                }
                // And the *geometry* is identical across states, so swapping the
                // frame does not move the button.
                for index in 0..a.len() {
                    assert_eq!(
                        a[index].rect, b[index].rect,
                        "quad {index} moved between {state_a:?} and {state_b:?}"
                    );
                }
            }
        }

        // The frames are the ones that were asked for, in the order they were
        // asked for: frame `n` starts at u = n * 16 / 64.
        for (index, state) in STATES.into_iter().enumerate() {
            let u0 = per_state[index][0].uv[0];
            let expected = (index as u32 * FRAME_SIZE) as f32 / SHEET_SIZE as f32;
            assert!(
                (u0 - expected).abs() < 1e-6,
                "{state:?} sampled frame at u={u0}, not the frame {index} at {expected}"
            );
        }
    }

    /// The insets a button lays out with are the ones the art expands with.
    #[test]
    fn the_insets_a_button_lays_out_with_come_from_the_art() {
        let skin = skin();
        assert!(skin.insets_agree());
        let insets = skin.insets();
        assert_eq!(
            insets,
            SkinInsets::new(
                NINE.left as f32,
                NINE.right as f32,
                NINE.top as f32,
                NINE.bottom as f32
            )
        );

        // The button's minimum is at least the art's, on both axes — a button
        // that could be laid out smaller than the frame's corners would smudge
        // them however careful the expansion was.
        //
        // Measured with **no label and no padding**, deliberately. A "Play" at
        // the default padding is already 36 px wide, which clears this skin's
        // 8-px corners on its own: the assertion then passes however wrong the
        // corner term is, and it did — shrinking the corner term to a quarter
        // left this green until the label stopped propping it up.
        let atlas = FontAtlas::built_in();
        let (art_w, art_h) = skin.idle.minimum_size();
        let mut bare = Button::new("").with_skin(insets);
        bare.padding = Vec2::ZERO;
        let bare_minimum = bare.minimum_size(&atlas);
        assert!(
            bare_minimum.x >= art_w && bare_minimum.y >= art_h,
            "an empty, unpadded button's minimum {bare_minimum:?} is under the \
             art's own ({art_w}, {art_h}) — the corners have nowhere to go"
        );

        let button = Button::new("Play").with_skin(insets);
        let minimum = button.minimum_size(&atlas);
        assert!(
            minimum.x >= bare_minimum.x && minimum.y >= bare_minimum.y,
            "a label may only ever make a button bigger"
        );

        // Drawn at its own minimum, the skin still emits a full nine: the button
        // never asks the expansion for the squashed-corner path.
        let target = [0.0, 0.0, minimum.x, minimum.y];
        for state in STATES {
            assert_eq!(
                skin.quads(state, target).len(),
                9,
                "{state:?} lost a band at the button's minimum size"
            );
        }
    }

    /// A missing frame is refused rather than silently falling back.
    #[test]
    fn a_skin_missing_a_frame_is_refused() {
        let art = art();
        assert!(ButtonSkin::from_sheet(SheetId(0), &art, 0, 1, 9).is_none());
        assert!(ButtonSkin::from_sheet(SheetId(0), &art, 7, 1, 2).is_none());

        let mut no_nine = art.clone();
        no_nine.nine = None;
        assert!(ButtonSkin::from_sheet(SheetId(0), &no_nine, 0, 1, 2).is_none());

        // `uniform` is the deliberate one-frame skin, and it agrees with itself.
        let skin = ButtonSkin::uniform(SheetId(0), skin().idle);
        assert!(skin.insets_agree());
        for state in STATES {
            assert_eq!(skin.source(state), &skin.idle);
        }
    }

    // -----------------------------------------------------------------------
    // The coordinate flip
    // -----------------------------------------------------------------------

    /// Screen space is Y **down** and world space is Y **up**, so the target's
    /// low edge comes from the screen rect's `max.y`.
    ///
    /// The asymmetric rect is the point: a rect centred on the viewport survives
    /// a missing flip, and would be the one a careless test picked.
    #[test]
    fn a_screen_rect_becomes_a_world_target_with_y_flipped() {
        let viewport = (256u32, 192u32);
        // Well into the top-left quadrant, so every sign is exercised.
        let min = Vec2::new(32.0, 16.0);
        let max = Vec2::new(224.0, 176.0);

        let centred = screen_rect_to_target(min, max, viewport, [0.0, 0.0]);
        assert_eq!(
            centred,
            [-96.0, -80.0, 192.0, 160.0],
            "for a camera on the origin, screen x 32..224 / y 16..176 is world \
             x -96..96 / y -80..80"
        );

        // A camera whose world origin is the viewport's bottom-left instead.
        let corner = screen_rect_to_target(min, max, viewport, [128.0, 96.0]);
        assert_eq!(corner, [32.0, 16.0, 192.0, 160.0]);

        // The flip, stated on its own: a rect hugging the *top* of the screen
        // must land at the *top* of the world, not the bottom.
        let top = screen_rect_to_target(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            viewport,
            [0.0, 0.0],
        );
        let bottom = screen_rect_to_target(
            Vec2::new(0.0, 182.0),
            Vec2::new(10.0, 192.0),
            viewport,
            [0.0, 0.0],
        );
        assert!(
            top[1] > bottom[1],
            "the top of the screen must be the high world Y: {top:?} against {bottom:?}"
        );
        assert_eq!(top[1] + top[3], 96.0, "it reaches the top of the viewport");
        assert_eq!(bottom[1], -96.0, "and the other reaches the bottom");
    }

    /// A button laid out by `crcbl-ui` and drawn through the skin covers exactly
    /// the rect it hit-tests, in the right place on the screen.
    ///
    /// This is the join the two crates meet at, and it is the one a unit test on
    /// either side alone cannot see.
    #[test]
    fn a_buttons_hit_rect_and_its_drawn_skin_are_the_same_rectangle() {
        let atlas = FontAtlas::built_in();
        let skin = skin();
        let viewport = (256u32, 192u32);
        let button = Button::new("Play")
            .with_skin(skin.insets())
            .with_fixed_size(Vec2::new(160.0, 40.0));
        let pos = Vec2::new(20.0, 30.0);

        let (min, max) = button.rect(pos, &atlas);
        let target = screen_rect_to_target(min, max, viewport, [0.0, 0.0]);
        let quads = skin.quads(ButtonState::Hovered, target);
        assert_eq!(quads.len(), 9);

        // The union of the quads is the target, and the target is the hit rect
        // mapped through the flip — so what is clicked is what is drawn.
        let low_x = quads.iter().fold(f32::MAX, |a, q| a.min(q.rect[0]));
        let high_x = quads
            .iter()
            .fold(f32::MIN, |a, q| a.max(q.rect[0] + q.rect[2]));
        let low_y = quads.iter().fold(f32::MAX, |a, q| a.min(q.rect[1]));
        let high_y = quads
            .iter()
            .fold(f32::MIN, |a, q| a.max(q.rect[1] + q.rect[3]));
        assert_eq!((low_x, high_x), (target[0], target[0] + target[2]));
        assert_eq!((low_y, high_y), (target[1], target[1] + target[3]));

        // Back the other way: the centre of the drawn skin is the point that
        // hit-tests as the centre of the button.
        let centre_screen = (min + max) * 0.5;
        assert!(button.hit_test(pos, centre_screen, &atlas));
        let centre_world = Vec2::new((low_x + high_x) * 0.5, (low_y + high_y) * 0.5);
        let half = Vec2::new(viewport.0 as f32, viewport.1 as f32) * 0.5;
        let round_tripped = Vec2::new(centre_world.x + half.x, half.y - centre_world.y);
        assert!(
            (round_tripped - centre_screen).abs().max_element() < 1e-4,
            "the drawn centre {round_tripped:?} is not the clicked centre \
             {centre_screen:?}"
        );

        // The label sits inside the drawn skin's content, not over its corners.
        //
        // Corners **plus** padding, not `>= the corners`: with this skin the
        // padding (8, 4) is wider than the left inset (3), so a content box that
        // had forgotten the corners entirely would still clear them and the
        // assertion would pass on a broken inset. Measured — dropping the inset
        // from `Button::content_rect` left this test green until it was written
        // as the equality it always meant.
        let insets = skin.insets();
        let (content_min, content_max) = button.content_rect(pos, &atlas);
        let expected_min = min + Vec2::new(insets.left, insets.top) + button.padding;
        let expected_max = max - Vec2::new(insets.right, insets.bottom) - button.padding;
        assert!(
            (content_min - expected_min).abs().max_element() < 1e-4,
            "the content box starts at {content_min:?}, not at the corners plus \
             the padding {expected_min:?}"
        );
        assert!(
            (content_max - expected_max).abs().max_element() < 1e-4,
            "the content box ends at {content_max:?}, not {expected_max:?}"
        );
        let label = button.label_pos(pos, &atlas);
        assert!(label.x >= content_min.x - 1e-4 && label.y >= content_min.y - 1e-4);
    }
}
