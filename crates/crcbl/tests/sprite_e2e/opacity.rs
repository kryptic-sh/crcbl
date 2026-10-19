//! Opacity groups through the UI pass: a group is faded as one layer, so its
//! top command covers its bottom one before the fade, and nested groups
//! multiply.
//!
//! The draw-list half and the plan of passes are unit-tested without a GPU;
//! this is the only place the layer is drawn, blended back and read off a real
//! frame — the premultiplied blend, the `Rgba16Float` layer and its
//! transitions on whichever backend `CRCBL_GPU` names.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;

use crate::harness::Headless;
use crate::sprite::{SPRITE_CLEAR, SPRITE_EXTENT, close, render_ui, rgb, srgb_byte};

/// The bottom command's colour, in linear light and opaque.
const BOTTOM: [f32; 4] = [0.80, 0.10, 0.05, 1.0];

/// The top command's colour, overlapping the bottom one's right half and
/// running past it.
const TOP: [f32; 4] = [0.05, 0.60, 0.20, 1.0];

/// The nested group's one command.
const NESTED: [f32; 4] = [0.90, 0.85, 0.10, 1.0];

/// The opacity of every group here but the dark one: one group at it is a
/// half, two nested are a quarter.
const OPACITY: f32 = 0.5;

/// A grey halfway between two eight-bit steps of linear light, so a layer
/// that stored linear colour in eight bits would round it a whole step away.
const DARK: [f32; 4] = [1.5 / 255.0, 1.5 / 255.0, 1.5 / 255.0, 1.0];

/// The dark group's opacity: high, so the rounding an eight-bit layer would
/// do is not faded below what the frame can show.
const DARK_OPACITY: f32 = 0.9;

/// Opaque black, which the dark group is blended over.
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// How far a channel may sit from its prediction: the blend in linear light,
/// the half-float layer and the sRGB encode on store each round once.
const SLACK: i32 = 2;

/// `colour` blended over the clear at `alpha`, in linear light, as the bytes
/// the `Rgba8UnormSrgb` frame stores.
fn over_clear(colour: [f32; 4], alpha: f32) -> [u8; 3] {
    over(SPRITE_CLEAR, colour, alpha)
}

fn over(below: [f32; 4], colour: [f32; 4], alpha: f32) -> [u8; 3] {
    let blended = blend(below, colour, alpha);
    [
        srgb_byte(blended[0]),
        srgb_byte(blended[1]),
        srgb_byte(blended[2]),
    ]
}

fn blend(below: [f32; 4], colour: [f32; 4], alpha: f32) -> [f32; 4] {
    let mut out = [0.0; 4];
    for channel in 0..3 {
        out[channel] = colour[channel] * alpha + below[channel] * (1.0 - alpha);
    }
    out[3] = 1.0;
    out
}

/// **A faded group's top command hides its bottom one**: where the two overlap
/// the frame is the clear blended with the top colour alone at the group's
/// opacity, exactly as it is where the top runs past the bottom, and not the
/// per-command alpha's top-over-faded-bottom — and the bottom alone is the
/// clear blended with the bottom colour. A group nested in another is faded
/// by both opacities multiplied, and a dark group keeps the precision of the
/// linear colour it was drawn in.
#[test]
#[ignore = "needs a real GPU and a backend pin; run tests/run-sprite-e2e.sh"]
fn a_faded_group_composites_as_one_layer_and_nested_groups_multiply() {
    let headless = Headless::open_for_sprites();
    let mut pool = crcbl::render::TransientPool::new();
    let mut ui =
        crcbl::render::UiRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the UI renderer builds");

    let mut list = DrawList::new();
    list.push_opacity(OPACITY);
    list.rect(Vec2::new(32.0, 32.0), Vec2::new(160.0, 112.0), BOTTOM);
    list.rect(Vec2::new(96.0, 56.0), Vec2::new(224.0, 88.0), TOP);
    list.pop_opacity().expect("pushed above");
    list.push_opacity(OPACITY);
    list.push_opacity(OPACITY);
    list.rect(Vec2::new(32.0, 136.0), Vec2::new(96.0, 168.0), NESTED);
    list.pop_opacity().expect("pushed above");
    list.pop_opacity().expect("pushed above");
    list.rect(Vec2::new(128.0, 128.0), Vec2::new(224.0, 176.0), BLACK);
    list.push_opacity(DARK_OPACITY);
    list.rect(Vec2::new(144.0, 136.0), Vec2::new(208.0, 168.0), DARK);
    list.pop_opacity().expect("pushed above");

    let (image, labels) = render_ui(&headless, &mut ui, &mut pool, SPRITE_EXTENT, &list);

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(
            labels,
            [
                "ui background",
                "ui-opacity",
                "ui-composite",
                "ui-opacity",
                "ui-opacity",
                "ui-composite",
                "ui-opacity",
                "ui-composite",
            ],
            "each group into its layer, the nested one inside its parent's, each blended back"
        );
        let check = |(x, y): (u32, u32), want: [u8; 3], what: &str| {
            let got = rgb(&image, x, y);
            assert!(
                close(got, want, SLACK),
                "({x}, {y}), {what}: {got:?}, not {want:?}"
            );
        };
        let bottom = over_clear(BOTTOM, OPACITY);
        let top = over_clear(TOP, OPACITY);
        check((48, 100), bottom, "the bottom alone");
        check(
            (128, 72),
            top,
            "the overlap, which only the top colour reaches",
        );
        check((200, 72), top, "the top alone, past the bottom");
        check((8, 8), over_clear(SPRITE_CLEAR, 1.0), "nothing drawn");
        check((64, 152), over_clear(NESTED, OPACITY * OPACITY), "nested");
        check(
            (136, 172),
            over(BLACK, BLACK, 1.0),
            "the black under the dark group",
        );
        check(
            (176, 152),
            over(BLACK, DARK, DARK_OPACITY),
            "the dark group, which an eight-bit linear layer would band",
        );

        // What multiplying each command's alpha draws at the overlap: the top
        // at a half over the bottom at a half. It must not be what the frame
        // shows, or this test could not tell a group from no group.
        let per_command = over(blend(SPRITE_CLEAR, BOTTOM, OPACITY), TOP, OPACITY);
        let got = rgb(&image, 128, 72);
        assert!(
            !close(got, per_command, 8),
            "the overlap is {got:?}, which is per-command alpha's {per_command:?}: the \
             bottom shows through the top"
        );
    }));

    ui.destroy(headless.device.as_ref());
    pool.destroy(headless.device.as_ref());
    headless.finish();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
