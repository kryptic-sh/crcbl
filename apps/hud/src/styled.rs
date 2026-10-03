//! The styled half of the page: the vitals panel, the minimap frame and the
//! wave banner, as a `crcbl_ui::tree` the stylesheet in `assets/hud.css` styles.
//!
//! ```text
//!  ┌ #vitals ────────┐       ┌ #banner ┐        ┌ #minimap ┐
//!  │ HEALTH ▰▰▰▰▱▱▱▱ │                          │ MAP      │
//!  │ MANA   ▰▰▱▱▱▱▱▱ │                          │          │
//!  └─────────────────┘                          └──────────┘
//!  ┌ the sheet's error, while there is one ┐
//! ```
//!
//! # The tree says what is there, the sheet says how it looks
//!
//! `build` is the whole tree, and every declaration in it is data rather than
//! look: the surface's size, and how full each bar is. Colours, sizes, borders
//! and positions are the sheet's, so restyling the HUD is an edit to
//! `hud.css` and no recompile — the web-workflow claim
//! `docs/plan/sample/04-hud.md` makes for this sample. The ability row and the
//! damage ticker are still [`crate::page`]'s draw-list primitives.
//!
//! # The error is not styled by the sheet that failed
//!
//! A save that does not parse keeps the last good sheet (see [`crate::sheet`])
//! and the refusal is drawn under the vitals panel in fixed colours, with the
//! draw list's own primitives. Styling it through the tree would let a sheet
//! hide its own error — `color: transparent` on the wrong selector is one save
//! away.

use std::time::Duration;

use crcbl::assets::AssetSource;
use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::style::Declaration;
use crcbl::ui::text::{FontAtlas, LINE_HEIGHT};
use crcbl::ui::tree::{AvailableSpace, LengthAuto, NodeKey, Ui};
use crcbl::ui::widget::{NATURAL_FONT_SIZE, PointerInput};

use crate::game::{HEALTH_MAX, MANA_MAX, RenderState};
use crate::sheet::LiveSheet;

/// The gap between the bottom of the vitals panel and the sheet's error under
/// it, in pixels.
const ERROR_GAP: f32 = 12.0;
/// Where the sheet's error is drawn when no vitals panel was laid out to hang
/// it under — a sheet that set `display: none` on it.
const ERROR_FALLBACK: Vec2 = Vec2::new(24.0, 24.0);
/// Padding between the error's backing and its text, in pixels.
const ERROR_PAD: f32 = 6.0;
/// The error's text size.
const ERROR_SIZE: f32 = 13.0;
/// The error's backing, in the draw list's linear light: a dark red.
const ERROR_BG: [f32; 4] = [0.18, 0.02, 0.02, 0.92];
/// The error's text: a light red that reads on [`ERROR_BG`].
const ERROR_TEXT: [f32; 4] = [1.0, 0.55, 0.50, 1.0];

/// The tree, its stylesheet, and the clock the sheet is polled on.
#[derive(Debug)]
pub struct StyledHud {
    ui: Ui,
    sheet: LiveSheet,
    /// The frame clock: every frame's [`crcbl::engine::FrameInfo::render_dt`],
    /// summed. A paused frame advances it, so a save made with the pause menu
    /// up is drawn while it is up.
    clock: Duration,
}

impl StyledHud {
    /// A tree styled by the sheet `source` holds under
    /// [`crate::sheet::SHEET_KEY`].
    pub fn new(source: Box<dyn AssetSource>) -> Self {
        let mut ui = Ui::new();
        let sheet = LiveSheet::new(&mut ui, source);
        Self {
            ui,
            sheet,
            clock: Duration::ZERO,
        }
    }

    /// Polls the sheet, then lays out and emits the tree for `state` into
    /// `dl`, and the sheet's error under it while there is one.
    ///
    /// `render_dt` is the wall-clock time this frame covers; `atlas` must be
    /// the one the UI pass renders with, for the reason [`crate::page::draw`]
    /// gives.
    pub fn draw(
        &mut self,
        dl: &mut DrawList,
        screen: Vec2,
        atlas: &FontAtlas,
        state: &RenderState,
        render_dt: Duration,
    ) {
        self.clock += render_dt;
        self.sheet.poll(&mut self.ui, self.clock);

        // No pointer: the page takes no input, so nothing on it is hovered.
        self.ui.begin_frame(PointerInput::default());
        let vitals = build(&mut self.ui, screen, state);
        self.ui
            .layout(Vec2::ZERO, AvailableSpace::definite(screen), atlas);
        self.ui.emit(dl);

        if let Some(error) = self.sheet.error() {
            // Under the panel wherever the sheet put it, so a sheet that moves
            // the panel does not move it over its own error.
            let origin = self
                .ui
                .rect(vitals)
                .filter(|(min, max)| max.y > min.y)
                .map_or(ERROR_FALLBACK, |(min, max)| {
                    Vec2::new(min.x, max.y + ERROR_GAP)
                });
            draw_error(dl, atlas, origin, error);
        }
    }

    /// Why the sheet showing is not the one in the source, while it is not.
    #[must_use]
    pub fn sheet_error(&self) -> Option<&str> {
        self.sheet.error()
    }
}

/// The whole tree: the surface, the vitals panel, the minimap's frame, and
/// the banner while the ticker raises it. Returns the vitals panel's key.
fn build(ui: &mut Ui, screen: Vec2, state: &RenderState) -> NodeKey {
    let surface = [
        Declaration::Width(LengthAuto::Px(screen.x)),
        Declaration::Height(LengthAuto::Px(screen.y)),
    ];
    let mut vitals = None;
    ui.block("#hud", &surface, |ui| {
        let panel = ui.block("#vitals", &[], |ui| {
            let health = format!("{} / {HEALTH_MAX}", state.health);
            bar(
                ui,
                "#health.bar",
                "HEALTH",
                state.health_fraction(),
                &health,
            );
            let mana = format!("{} / {MANA_MAX}", state.mana);
            bar(ui, "#mana.bar", "MANA", state.mana_fraction(), &mana);
        });
        vitals = Some(panel.key);
        ui.block("#minimap", &[], |ui| {
            ui.span(".caption", "MAP", &[]);
        });
        if state.banner {
            ui.block("#banner-row", &[], |ui| {
                ui.block("#banner", &[], |ui| {
                    ui.span("", format!("WAVE {}", state.wave).as_str(), &[]);
                });
            });
        }
    });
    vitals.expect("the surface's builder ran")
}

/// One bar: a caption over a track, the part of it that is full, and the
/// readout inside it.
///
/// The fill is built only when there is some, so an empty pool draws a track
/// and no zero-width block inside it.
fn bar(ui: &mut Ui, selector: &str, caption: &str, fraction: f32, value: &str) {
    ui.block(selector, &[], |ui| {
        ui.span(".caption", caption, &[]);
        ui.block(".track", &[], |ui| {
            let fraction = fraction.clamp(0.0, 1.0);
            if fraction > 0.0 {
                let width = [Declaration::Width(LengthAuto::Percent(fraction))];
                ui.block(".fill", &width, |_| {});
            }
            ui.span(".value", value, &[]);
        });
    });
}

/// The sheet's error, one line per line of it, on a backing sized to fit.
fn draw_error(dl: &mut DrawList, atlas: &FontAtlas, origin: Vec2, error: &str) {
    let scale = ERROR_SIZE / NATURAL_FONT_SIZE;
    let line_height = LINE_HEIGHT * scale;
    let width = error
        .lines()
        .map(|line| atlas.text_width(line, scale))
        .fold(0.0, f32::max);
    let lines = error.lines().count() as f32;
    let max = origin + Vec2::new(width, line_height * lines) + Vec2::splat(ERROR_PAD * 2.0);
    dl.rect(origin, max, ERROR_BG);
    for (row, line) in error.lines().enumerate() {
        let pos = origin + Vec2::new(ERROR_PAD, ERROR_PAD + line_height * row as f32);
        dl.text(pos, line, ERROR_TEXT, ERROR_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet::{BUILT_IN_CSS, SHEET_KEY, built_in_source};
    use crcbl::ui::draw_list::DrawCommand;
    use crcbl::ui::style::STYLESHEET_POLL_INTERVAL;

    const SCREEN: Vec2 = Vec2::new(960.0, 720.0);

    /// `#ff0000` and `#00ff00` decoded to linear light, which leaves both
    /// unchanged: nothing on the committed page is either.
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

    /// The committed sheet with the health bar's fill overridden: the one
    /// declaration these tests watch move.
    fn health_fill(colour: &str) -> String {
        format!("{BUILT_IN_CSS}\n#health .fill {{ background: {colour}; }}\n")
    }

    /// A directory under the system's temporary directory holding a
    /// `hud.css`, removed when dropped.
    struct TempStyles(std::path::PathBuf);

    impl TempStyles {
        fn new(test: &str, css: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("hud-{test}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("the temporary directory is made");
            let styles = Self(dir);
            styles.write(css);
            styles
        }

        fn write(&self, css: &str) {
            std::fs::write(self.0.join(SHEET_KEY), css).expect("the sheet is written");
        }

        fn source(&self) -> Box<dyn AssetSource> {
            Box::new(crcbl::assets::DirSource::at(self.0.clone()))
        }
    }

    impl Drop for TempStyles {
        fn drop(&mut self) {
            // A leftover directory in the temporary directory is harmless; a
            // panic here would hide the assertion that failed.
            if let Err(error) = std::fs::remove_dir_all(&self.0) {
                eprintln!("{}: not removed: {error}", self.0.display());
            }
        }
    }

    /// Health half full, mana full, the banner up on wave two.
    fn state() -> RenderState {
        RenderState {
            wave: 2,
            banner: true,
            health: HEALTH_MAX / 2,
            mana: MANA_MAX,
            ..RenderState::default()
        }
    }

    /// One frame of `state`, `dt` after the last, drawn into a fresh list.
    fn frame(hud: &mut StyledHud, state: &RenderState, dt: Duration) -> DrawList {
        let mut dl = DrawList::new();
        hud.draw(&mut dl, SCREEN, &FontAtlas::built_in(), state, dt);
        dl
    }

    /// One frame a whole poll interval after the last, so the sheet is read.
    fn polled(hud: &mut StyledHud) -> DrawList {
        frame(hud, &state(), STYLESHEET_POLL_INTERVAL)
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
    }

    /// Whether any filled rect on the list is painted `colour`.
    fn painted(dl: &DrawList, colour: [f32; 4]) -> bool {
        dl.commands().iter().any(
            |command| matches!(command, DrawCommand::Rect { color, .. } if close(*color, colour)),
        )
    }

    /// Every `Text` command on the list, in order.
    fn text_of(dl: &DrawList) -> Vec<String> {
        dl.commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The width of each bar's fill, in draw order, as a fraction of the inside
    /// of its track.
    ///
    /// Found by shape rather than by colour, so the claim holds whatever the
    /// sheet paints them: a track is a rect of the committed `.track` size, and
    /// its fill is the rect starting one border in from its corner.
    fn fill_fractions(dl: &DrawList) -> Vec<f32> {
        let rects: Vec<(Vec2, Vec2)> = dl
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Rect { min, max, .. } => Some((*min, *max)),
                _ => None,
            })
            .collect();
        let border = Vec2::ONE;
        rects
            .iter()
            .filter(|(min, max)| *max - *min == Vec2::new(280.0, 18.0))
            .map(|(track_min, track_max)| {
                let inside = *track_max - *track_min - border * 2.0;
                rects
                    .iter()
                    .find(|(min, _)| *min == *track_min + border)
                    .map_or(0.0, |(min, max)| (max.x - min.x) / inside.x)
            })
            .collect()
    }

    /// **The tree draws what the criterion names, in the committed sheet**:
    /// both bars filled to their fraction with their readouts, the minimap's
    /// frame, and the banner only while the ticker raises it.
    #[test]
    fn the_tree_draws_the_vitals_the_minimap_frame_and_the_banner() {
        let mut hud = StyledHud::new(Box::new(built_in_source()));
        let dl = polled(&mut hud);
        let text = text_of(&dl);
        let health = format!("{} / {HEALTH_MAX}", HEALTH_MAX / 2);
        let mana = format!("{MANA_MAX} / {MANA_MAX}");
        for line in ["HEALTH", "MANA", "MAP", "WAVE 2", &health, &mana] {
            assert!(text.iter().any(|t| t == line), "missing {line}: {text:?}");
        }
        assert_eq!(
            fill_fractions(&dl),
            vec![0.5, 1.0],
            "half health, full mana"
        );
        assert_eq!(hud.sheet_error(), None, "the committed sheet parses");

        let down = RenderState {
            banner: false,
            ..state()
        };
        let dl = frame(&mut hud, &down, STYLESHEET_POLL_INTERVAL);
        assert!(!text_of(&dl).iter().any(|t| t.starts_with("WAVE")));
    }

    /// **A changed sheet restyles the next polled frame, and not before**: no
    /// rebuild, no restart, one write to the file the source reads.
    #[test]
    fn writing_a_changed_sheet_restyles_the_hud_on_the_next_poll() {
        let styles = TempStyles::new("restyle", &health_fill("#ff0000"));
        let mut hud = StyledHud::new(styles.source());
        let dl = polled(&mut hud);
        assert!(
            painted(&dl, RED),
            "the directory's sheet was not read on the first frame"
        );

        styles.write(&health_fill("#00ff00"));
        let early = frame(&mut hud, &state(), Duration::ZERO);
        assert!(
            painted(&early, RED) && !painted(&early, GREEN),
            "the file was read inside the poll interval"
        );

        let dl = polled(&mut hud);
        assert!(
            painted(&dl, GREEN),
            "the saved sheet did not restyle the HUD"
        );
        assert!(!painted(&dl, RED), "the old fill is still drawn");
        assert_eq!(hud.sheet_error(), None);
    }

    /// **A save that does not parse keeps the last good sheet and puts the
    /// error on screen; the next good save takes over and clears it.**
    #[test]
    fn a_broken_sheet_keeps_the_last_good_one_and_shows_the_error_until_fixed() {
        let styles = TempStyles::new("broken", &health_fill("#ff0000"));
        let mut hud = StyledHud::new(styles.source());
        assert!(painted(&polled(&mut hud), RED));

        // The green rule parses; the sibling combinator after it does not, and
        // one error refuses the whole save.
        let broken = format!("{}.y + .z {{ width: 1px; }}\n", health_fill("#00ff00"));
        styles.write(&broken);
        let dl = polled(&mut hud);
        assert!(painted(&dl, RED), "the last good sheet was not kept");
        assert!(!painted(&dl, GREEN), "a refused save was drawn");

        let error = hud.sheet_error().expect("the refusal is kept").to_owned();
        assert!(error.contains("keeping the last good sheet"), "{error}");
        let text = text_of(&dl);
        for line in error.lines() {
            assert!(
                text.iter().any(|t| t == line),
                "the error line {line:?} is not on screen: {text:?}"
            );
        }
        assert!(
            text.iter()
                .any(|t| t.starts_with(&format!("{SHEET_KEY}:")) && t.contains(": error:")),
            "the error is not located at its file and line: {text:?}"
        );

        styles.write(&health_fill("#00ff00"));
        let dl = polled(&mut hud);
        assert!(painted(&dl, GREEN), "the fixed save was not taken");
        assert_eq!(hud.sheet_error(), None, "the fixed save left the error up");
        assert!(
            !text_of(&dl).iter().any(|t| t.contains("last good sheet")),
            "the error is still on screen"
        );
    }
}
