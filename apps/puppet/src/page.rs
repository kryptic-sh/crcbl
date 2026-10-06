//! The overlay: a readout panel and the control prompt, over the 3D frame.
//!
//! ```text
//!  ┌ puppet ──────────┐
//!  │ X        0.00 m  │
//!  │ Y        0.30 m  │
//!  │ Z       -2.71 m  │
//!  │ GROUND      YES  │
//!  │ PILOT    PLAYER  │
//!  │ SPEED   3.20 m/s │
//!  │ BLEND       1.00 │
//!  │ STATE        RUN │
//!  │ STEPS         14 │
//!  └──────────────────┘
//!
//!     WASD walk   Shift run   Space jump   Q/E turn the camera   R/F tilt it
//! ```
//!
//! The prompt is [`crate::bindings::prompt`]'s, for the device the player last
//! used — the line above on the keyboard, `Left stick walk   RB run   A jump`
//! and the camera keys once an Xbox pad speaks. This page only draws it.
//!
//! # It is small on purpose
//!
//! The subject of this sample is what the character is doing in the world, and
//! anything drawn over that is in the way of it. The panel carries the three
//! numbers a reader needs to check the frame against — where the feet are, and
//! whether the controller says they are on the ground — and the debug panel
//! (`F3`) carries the rest.
//!
//! # Laid out against the surface
//!
//! Every position is derived from the extent the swapchain was actually
//! acquired at, so the page is correct in a resized window and in the headless
//! offscreen ring at whatever `--size` asked for.

use crcbl::ui::draw_list::DrawList;
use crcbl::ui::readout::{ReadoutPanel, ReadoutRow};
use crcbl::ui::text::FontAtlas;

use crate::game::RenderState;

const PANEL_BG: [f32; 4] = [0.06, 0.07, 0.11, 0.80];
const BORDER: [f32; 4] = [0.34, 0.38, 0.48, 1.0];
const LABEL: [f32; 4] = [0.66, 0.70, 0.80, 1.0];
const VALUE: [f32; 4] = [0.95, 0.96, 1.0, 1.0];
/// What a row is drawn in when the controller is refusing the move — the one
/// piece of state on this panel that is worth a colour.
const REFUSED: [f32; 4] = [0.95, 0.55, 0.36, 1.0];

/// The panel the readings are drawn in: this page's geometry and palette, over
/// [`crcbl::ui::readout`]'s layout.
const PANEL: ReadoutPanel = ReadoutPanel {
    inset: 16.0,
    // Wide enough for the longest label and a right-aligned reading beside it.
    width: 168.0,
    row_height: 18.0,
    pad: 8.0,
    border_width: 1.0,
    background: PANEL_BG,
    border: BORDER,
    label: LABEL,
};

/// What the page drew, for the loop's own tests and its summary line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PageStats {
    /// How many draw commands the page produced.
    pub commands: usize,
}

/// Draws the overlay into `list`, laid out against a surface of `extent`.
///
/// `atlas` is only measured against — the glyphs themselves are the UI pass's
/// business — and it is what right-aligns the readings against a proportional
/// font rather than against a guess; see [`ReadoutPanel::draw_at`].
/// `blend` is how far the character is out of its idle stance —
/// [`crate::anim::Animator::blend`] — and `anim` the name of the state machine
/// state the pose was sampled in. Both arrive as arguments rather than on
/// `state` because they are the client's reading of the pose: the simulation
/// sends the machine's state, and the names and weights are what the client's
/// copy of the asset makes of it. `prompt` is the control prompt, which is the
/// whole of what a first-time visitor needs.
pub fn draw(
    list: &mut DrawList,
    atlas: &FontAtlas,
    extent: (u32, u32),
    state: &RenderState,
    blend: f32,
    anim: &str,
    prompt: &str,
) -> PageStats {
    let rows: [ReadoutRow; 9] = [
        ReadoutRow::new("X", format!("{:.2} m", state.position.x), VALUE),
        ReadoutRow::new("Y", format!("{:.2} m", state.feet), VALUE),
        ReadoutRow::new("Z", format!("{:.2} m", state.position.z), VALUE),
        ReadoutRow::new(
            "GROUND",
            if state.grounded { "YES" } else { "NO" },
            if state.grounded { VALUE } else { REFUSED },
        ),
        ReadoutRow::new(
            "PILOT",
            if state.patrolling {
                "CIRCUIT"
            } else {
                "PLAYER"
            },
            if state.blocked { REFUSED } else { VALUE },
        ),
        // The two numbers milestone 2 is about, side by side: what the world
        // let the character do, and which pose that selected. Reading them
        // together is the eyeball check that the blend tracks the body.
        ReadoutRow::new("SPEED", format!("{:.2} m/s", state.speed), VALUE),
        ReadoutRow::new("BLEND", format!("{blend:.2}"), VALUE),
        // Which state the machine picked, and the footsteps its run raised —
        // the one animation event this sample has, counted on the server.
        ReadoutRow::new("STATE", anim.to_uppercase(), VALUE),
        ReadoutRow::new("STEPS", format!("{}", state.footsteps), VALUE),
    ];

    PANEL.draw(list, atlas, &rows);
    PANEL.hint(list, atlas, extent, prompt);

    PageStats {
        commands: list.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::math::{DVec3, Vec2};
    use crcbl::ui::readout::NATURAL_SCALE;
    use crcbl::ui::widget::NATURAL_FONT_SIZE;

    /// A prompt to draw: the keyboard's, as a run opens on.
    fn keyboard_prompt() -> String {
        crate::bindings::prompt(&crate::bindings::action_map())
    }

    /// The prompt for a pad whose family nobody could place, which prints the
    /// longest words the label table has for these bindings — the widest line
    /// the page is ever handed.
    fn generic_pad_prompt() -> String {
        use crcbl::input::{GamepadEvent, GamepadId, GamepadSnapshot, PadButton, PadKind};
        let mut actions = crate::bindings::action_map();
        let mut snapshot = GamepadSnapshot::neutral(PadKind::Generic);
        snapshot.buttons.insert(PadButton::South);
        actions.gamepad_event(&GamepadEvent::State {
            id: GamepadId(1),
            snapshot,
        });
        crate::bindings::prompt(&actions)
    }

    /// **The page draws something, and what it draws says where the character
    /// is.** A frame with an empty draw list is the one failure a headless
    /// smoke test would otherwise report as a pass.
    #[test]
    fn the_panel_carries_the_position_the_frame_was_drawn_at() {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let state = RenderState {
            position: DVec3::new(1.25, 0.9, -2.5),
            feet: 0.3,
            grounded: true,
            speed: 2.5,
            footsteps: 14,
            ..RenderState::default()
        };
        let prompt = keyboard_prompt();
        let stats = draw(&mut list, &atlas, (960, 720), &state, 0.78, "run", &prompt);
        assert!(stats.commands > 0, "the page drew nothing at all");

        let text: Vec<&str> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                crcbl::ui::draw_list::DrawCommand::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            text.contains(&"1.25 m"),
            "the X reading is missing: {text:?}"
        );
        assert!(
            text.contains(&"0.30 m"),
            "the Y reading is the feet, not the capsule centre: {text:?}"
        );
        assert!(
            text.contains(&"-2.50 m"),
            "the Z reading is missing: {text:?}"
        );
        assert!(
            text.contains(&"YES"),
            "the ground reading is missing: {text:?}"
        );
        assert!(
            text.contains(&prompt.as_str()),
            "the control prompt is missing: {text:?}"
        );
        // The two milestone-2 readings: the speed the world allowed, and the
        // blend weight it selected.
        assert!(
            text.contains(&"2.50 m/s"),
            "the speed reading is missing: {text:?}"
        );
        assert!(
            text.contains(&"0.78"),
            "the blend reading is missing: {text:?}"
        );
        assert!(
            text.contains(&"RUN"),
            "the animation state is missing: {text:?}"
        );
        assert!(
            text.contains(&"14"),
            "the footstep count is missing: {text:?}"
        );
    }

    /// **A reading that is measured wrong is drawn off the surface, and the
    /// draw list still contains it.** Asserting the strings reach the list says
    /// nothing about where they land, so this asserts the geometry: every
    /// string the page emits starts inside the surface it was laid out
    /// against, and the right-aligned readings start inside their own panel.
    #[test]
    fn every_reading_is_laid_out_where_it_can_actually_be_seen() {
        for prompt in [keyboard_prompt(), generic_pad_prompt()] {
            laid_out_where_it_can_be_seen(&prompt);
        }
    }

    /// [`every_reading_is_laid_out_where_it_can_actually_be_seen`] for one
    /// prompt.
    fn laid_out_where_it_can_be_seen(prompt: &str) {
        let atlas = FontAtlas::built_in();
        let mut list = DrawList::new();
        let state = RenderState {
            position: DVec3::new(-12.75, 0.9, -2.5),
            feet: 0.3,
            grounded: false,
            ..RenderState::default()
        };
        let extent = (960u32, 720u32);
        draw(&mut list, &atlas, extent, &state, 0.5, "jump", prompt);

        let drawn: Vec<(Vec2, &str)> = list
            .commands()
            .iter()
            .filter_map(|command| match command {
                crcbl::ui::draw_list::DrawCommand::Text { pos, text, .. } => {
                    Some((*pos, text.as_str()))
                }
                _ => None,
            })
            .collect();
        assert!(!drawn.is_empty(), "the page drew no text at all");

        for (pos, text) in &drawn {
            assert!(
                pos.x >= 0.0 && pos.y >= 0.0,
                "{text:?} starts off the top-left corner at {pos:?}"
            );
            let end = pos.x + atlas.text_width(text, NATURAL_SCALE);
            assert!(
                end <= extent.0 as f32 && pos.y + NATURAL_FONT_SIZE <= extent.1 as f32,
                "{text:?} runs off a {extent:?} surface: {pos:?}..{end}"
            );
        }

        let panel_right = PANEL.inset + PANEL.width;
        for (pos, text) in drawn.iter().filter(|(_, text)| *text != prompt) {
            assert!(
                pos.x >= PANEL.inset
                    && pos.x + atlas.text_width(text, NATURAL_SCALE) <= panel_right,
                "{text:?} is outside the panel's {}..{panel_right} columns: {pos:?}",
                PANEL.inset,
            );
        }
    }
}
