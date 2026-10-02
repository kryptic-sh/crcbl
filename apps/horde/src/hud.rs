//! Horde's HUD: the stat line and the state line over the field, and the band
//! behind them.

use crcbl::math::Vec2;
use crcbl::ui::draw_list::DrawList;

use crate::game::{GameState, RenderState};

/// The HUD's lines, rebuilt only when the numbers behind them change.
///
/// `DrawList::text` needs an owned `String`, so the alternative is a `format!`
/// per line every frame at whatever rate the window runs. The clock is keyed on
/// **tenths of a second** rather than on the raw `f64`, which is both what the
/// line shows and the only way an `f64` can be part of an `Eq` key.
#[derive(Debug, Default)]
pub(crate) struct HudStrings {
    stats: String,
    state: String,
    last: Option<HudKey>,
}

type HudKey = (u64, u64, u32, u32, u64, usize, u32, Option<GameState>, bool);

/// `seconds` as `m:ss`.
fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

impl HudStrings {
    /// **`paused` wins over the simulation's state**, which is the bug flappy
    /// fixed: the status line used to read straight off the *server's* idea of
    /// what was happening, and the server is still playing while the window sits
    /// behind a browser.
    pub(crate) fn refresh(&mut self, render: &RenderState, paused: bool) {
        let key = (
            (render.elapsed * 10.0) as u64,
            render.kills,
            render.player_hp.max(0.0) as u32,
            render.level,
            render.xp,
            render.enemies.len(),
            render.best,
            render.state,
            paused,
        );
        if self.last == Some(key) {
            return;
        }
        self.last = Some(key);

        use std::fmt::Write as _;
        self.stats.clear();
        let _ = write!(
            self.stats,
            "{}   Kills: {}   HP: {:.0}/{:.0}   Lv {} ({}/{})   Enemies: {}",
            clock(render.elapsed),
            render.kills,
            render.player_hp.max(0.0),
            render.player_max_hp,
            render.level,
            render.xp,
            render.xp_needed,
            render.enemies.len(),
        );
        self.state.clear();
        // **The record lives on the second line, not the first**, and that is a
        // width decision rather than a taste one: the stat line already runs to
        // most of a 960-pixel window at the counts this game reaches, and the
        // second line is short in every state. `the_hud_fits_the_panel_it_is_drawn_on`
        // is what holds both of them.
        let _ = write!(self.state, "Best {}   ", clock(f64::from(render.best)));
        if paused {
            self.state.push_str("PAUSED - press ESC");
        } else {
            match render.state {
                Some(GameState::WaitingToStart) | None => {
                    self.state.push_str("PRESS SPACE TO PLAY - WASD to move");
                }
                Some(GameState::Dead) => {
                    let _ = write!(
                        self.state,
                        "YOU DIED - survived {}, {} kills - press R",
                        clock(render.elapsed),
                        render.kills,
                    );
                }
                Some(GameState::LevelUp) => {
                    self.state.push_str("LEVEL UP - press 1, 2 or 3");
                }
                Some(GameState::Playing) => {
                    self.state.push_str("WASD to move - the gun aims itself");
                }
            }
        }
    }
}

/// Draws the HUD, and nothing else.
///
/// **No scrim here any more.** The placeholder renderer dimmed the field behind
/// its death screen by hand; `crcbl::ui::menu::Menu::render` draws one behind every
/// menu, so a second would dim the field twice on exactly the frames a menu is
/// up.
pub(crate) fn draw_hud(dl: &mut DrawList, hud: &HudStrings) {
    dl.rect(
        HUD_ORIGIN,
        Vec2::new(HUD_PANEL_RIGHT, 52.0),
        [0.1, 0.1, 0.15, 0.85],
    );
    dl.text(
        Vec2::new(HUD_TEXT_X, 10.0),
        hud.stats.as_str(),
        [1.0, 1.0, 0.3, 1.0],
        HUD_STAT_SIZE,
    );
    dl.text(
        Vec2::new(HUD_TEXT_X, 32.0),
        hud.state.as_str(),
        [0.7, 0.7, 1.0, 1.0],
        HUD_STATE_SIZE,
    );
}

/// The HUD backdrop's top-left corner, in framebuffer pixels.
const HUD_ORIGIN: Vec2 = Vec2::new(4.0, 4.0);
/// Where the backdrop ends. See [`HUD_STAT_SIZE`].
///
/// `pub(crate)` for one reason: `crate::controls` puts the pause button in the
/// top-right corner and asserts it clears this, so the two layouts are held
/// apart by the number itself rather than by two people eyeballing a screenshot.
pub(crate) const HUD_PANEL_RIGHT: f32 = 820.0;
/// Where both lines of text start.
const HUD_TEXT_X: f32 = 10.0;
/// The stat line's font size, and the reason the panel is as wide as it is.
///
/// **The width is measured, not guessed** — `the_hud_fits_the_panel_it_is_drawn_on`
/// puts a stated worst-case run through the real
/// [`crcbl::ui::text::FontAtlas`] and requires it inside [`HUD_PANEL_RIGHT`].
/// The placeholder renderer's 430 became 560 became 690 by eye, and the browser
/// gate's capture caught the last of those with the text running off the end of
/// its own backdrop.
///
/// What is bounded is a five-minute run at the shipped enemy cap. `--max-enemies
/// 10000` with a twenty-minute soak behind it can still outgrow this; that is
/// recorded in `docs/backlog.md` rather than solved by a panel two thirds of the
/// window wide.
const HUD_STAT_SIZE: f32 = 16.0;
/// The state line's, which is smaller because it is prose.
const HUD_STATE_SIZE: f32 = 14.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game;

    /// The HUD reports the pause rather than the simulation's state, and the
    /// death line says how the run ended.
    #[test]
    fn the_hud_says_paused_even_though_the_simulation_is_not() {
        let mut hud = HudStrings::default();
        let render = RenderState {
            state: Some(GameState::Playing),
            player_hp: game::PLAYER_MAX_HP,
            player_max_hp: game::PLAYER_MAX_HP,
            elapsed: 74.0,
            kills: 12,
            level: 3,
            xp: 5,
            xp_needed: 16,
            ..RenderState::default()
        };
        hud.refresh(&render, false);
        assert!(hud.state.contains("WASD"), "{}", hud.state);
        assert!(hud.stats.contains("1:14"), "{}", hud.stats);
        assert!(hud.stats.contains("Kills: 12"), "{}", hud.stats);
        assert!(hud.stats.contains("Lv 3 (5/16)"), "{}", hud.stats);

        hud.refresh(&render, true);
        assert!(hud.state.contains("PAUSED"), "{}", hud.state);

        hud.refresh(
            &RenderState {
                state: Some(GameState::LevelUp),
                ..render.clone()
            },
            false,
        );
        assert!(hud.state.contains("LEVEL UP"), "{}", hud.state);

        hud.refresh(
            &RenderState {
                state: Some(GameState::Dead),
                elapsed: 133.0,
                kills: 208,
                ..render
            },
            false,
        );
        assert!(hud.state.contains("YOU DIED"), "{}", hud.state);
        assert!(hud.state.contains("2:13"), "{}", hud.state);
        assert!(hud.state.contains("208 kills"), "{}", hud.state);
    }

    /// **Both HUD lines fit the backdrop they are drawn on**, at a worst case
    /// this game can actually reach.
    ///
    /// Measured through the real [`crcbl::ui::text::FontAtlas`] — the same one
    /// the UI pass draws with — rather than by counting characters, and it is
    /// the check that was missing: the browser gate's canvas capture showed
    /// `Enemies: 9` sitting a hundred pixels past the end of its own panel, on a
    /// line three fields shorter than this one.
    #[test]
    fn the_hud_fits_the_panel_it_is_drawn_on() {
        use crcbl::ui::NATURAL_FONT_SIZE;
        let atlas = crcbl::ui::text::FontAtlas::built_in();

        // A five-minute run at the shipped cap, with every field at the widest
        // this game puts in it: nine hundred kills, six Vitality upgrades, a
        // level in double figures, and a full field.
        let render = RenderState {
            state: Some(GameState::Dead),
            player_hp: 0.0,
            player_max_hp: 250.0,
            elapsed: 300.0,
            kills: 2_048,
            level: 18,
            xp: 240,
            xp_needed: 1_024,
            best: 359,
            enemies: vec![
                game::EnemyView {
                    position: crcbl::math::DVec3::ZERO,
                    kind: game::EnemyKind::Grunt,
                    health: 1.0,
                };
                game::DEFAULT_MAX_ENEMIES
            ],
            ..RenderState::default()
        };
        let mut hud = HudStrings::default();
        hud.refresh(&render, false);

        for (line, size) in [(&hud.stats, HUD_STAT_SIZE), (&hud.state, HUD_STATE_SIZE)] {
            let right = HUD_TEXT_X + atlas.text_width(line, size / NATURAL_FONT_SIZE);
            assert!(
                right <= HUD_PANEL_RIGHT,
                "\"{line}\" ends at {right:.0} px, past the panel's {HUD_PANEL_RIGHT:.0}",
            );
        }
        // …and the panel is not simply enormous: it fits the window the game
        // opens at, which is what makes the assertion above a fit rather than a
        // licence.
        const { assert!(HUD_PANEL_RIGHT < 960.0) };
        // The record really is on the second line, or the width above is being
        // asserted about the wrong string.
        assert!(hud.state.starts_with("Best 5:59"), "{}", hud.state);
        assert!(!hud.stats.contains("Best"), "{}", hud.stats);
    }

    /// The clock is `m:ss`, including the cases a naive `{}:{}` gets wrong.
    #[test]
    fn the_clock_reads_as_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(9.9), "0:09");
        assert_eq!(clock(59.999), "0:59");
        assert_eq!(clock(60.0), "1:00");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(605.0), "10:05");
        assert_eq!(clock(-1.0), "0:00", "a clock never runs backwards");
    }
}
