//! Sundial's menu: the pause panel, and the rows of its own.
//!
//! Smaller than a game's, for `apps/alcove/src/menu.rs`' reason: this is a
//! fixture, not a game — there is no run to start and nothing to win, so a
//! `GAME OVER` panel would be a screen it could never show.
//!
//! What it does have is every control `docs/plan/sample/18-sundial.md`'s
//! milestone 1 asks to be *legible*: which filter is running, where the
//! comparison seam stands and — because that is the whole of what the sample adds
//! to the engine's half — **which filter each side of the seam is**, plus the sun
//! and its clock.
//!
//! # Two kinds of row, and the difference is deliberate
//!
//! A row with a [`SundialAction`] behind it is pressed with `ENTER`, and its hint
//! says so. A row without one is a **reading**: the seam's position and the sun's
//! tick each move in two directions, so their control is a *pair* of keys rather
//! than a button, and the hint column names the pair. A press on such a row does
//! nothing, on purpose — one `ENTER` cannot say which of the two directions was
//! meant.
//!
//! `docs/plan/sample/00-samples-overview.md` rule 4 is why there is a panel at
//! all, and the debug overlay's `shadow filter` and `sun` sections carry the same
//! readings for a run nobody has paused.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::render::{DebugView, EffectRequest, RenderEffects};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::filter::Knobs;
use crate::sun::Clock;

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The pose [`crate::plaza::fixed_camera`] names, held still.
    ///
    /// The default, and deliberately: a run whose first frame is the golden's
    /// frame is one whose screenshot can be compared to the checked-in reference
    /// without anybody having to stand in the right place first.
    #[default]
    Fixed,
    /// The pose [`crate::plaza::counter_camera`] names, also held still.
    ///
    /// **A mode rather than a golden-only pose**, unlike `apps/alcove`'s rim
    /// camera: the penumbra ladder is the one claim in this fixture that cannot
    /// be judged from the fixed pose at all — the counters' shadows are a few
    /// dozen pixels across there — so a reviewer who cannot reach this pose
    /// cannot look at the thing the scene was laid out for.
    Counters,
    /// [`crcbl::render::Flyer`], starting at the fixed pose.
    Free,
}

impl CameraMode {
    /// Parses `fixed` / `counters` / `free`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "fixed" | "golden" => Some(Self::Fixed),
            "counters" | "ladder" => Some(Self::Counters),
            "free" | "fly" | "free-fly" => Some(Self::Free),
            _ => None,
        }
    }

    /// The next one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Fixed => Self::Counters,
            Self::Counters => Self::Free,
            Self::Free => Self::Fixed,
        }
    }

    /// What the panel and the summary call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fixed => "FIXED",
            Self::Counters => "COUNTERS",
            Self::Free => "FREE",
        }
    }
}

/// The actions this sample's menus have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SundialAction {
    /// Move on to the next camera, and put the free one back at the fixed pose
    /// when it comes round again.
    CycleCamera,
    /// Flip the shadow passes themselves, in the one layer of
    /// `docs/plan/39-capabilities.md`'s resolution order a panel owns.
    ///
    /// **The control for every claim this fixture makes.** With the passes off
    /// every surface is lit, so a reading that did not move when they went away
    /// was never about a shadow — and standing the two beside each other live is
    /// the fastest way to see which parts of the picture the shadow term owns.
    ToggleEffect(RenderEffects),
    /// Move `r_shadow_filter` on to the next rung the engine declares.
    CycleFilter,
    /// Show the cascade overlay, or take it away — `crcbl::debug_view`'s
    /// `DebugView::Cascades`.
    ///
    /// **The one row that changes the picture rather than the shadow.** Every
    /// other control here moves what the shadow *is*; this one leaves it alone
    /// and colours the frame by which cascade each sun-lit fragment read, which
    /// is the only way the cross-fade band `docs/plan/45-shadows.md`'s eighth
    /// decision added is a thing a reviewer can look at.
    ToggleCascades,
    /// Draw the shadow atlas over the frame, or take it away —
    /// `crcbl::debug_view`'s `DebugView::ShadowAtlas`.
    ///
    /// **The one row that is about the atlas rather than about the picture.**
    /// Every other control here — [`Self::ToggleCascades`] included — asks a
    /// question about the frame; this one replaces the frame with the `D32Float`
    /// image the shadow pass filled, which is the only way "which slot holds
    /// which map, and which slots hold nothing" is a thing a reviewer can look
    /// at rather than infer from a scene that looks lit either way.
    ToggleAtlas,
    /// Put the comparison seam up at [`crate::filter::SEAM_CENTRE`], or take it
    /// away.
    ToggleSeam,
    /// Start or stop the scripted clock.
    ToggleSun,
    /// Put every knob back and the clock back to the fixture tick.
    Reset,
}

/// The id carrying [`SundialAction::CycleCamera`]. The first id a game may use,
/// per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The shadow passes' own row.
pub const SHADOWS_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The filter selector's row.
pub const FILTER_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 2;

/// The comparison seam's row.
pub const SEAM_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 3;

/// The clock's run/stop row.
pub const SUN_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 4;

/// The row that puts everything back.
pub const RESET_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 5;

/// The reading naming the filter on the seam's **near** side — no action; see
/// this module's header.
pub const NEAR_SIDE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 6;

/// The reading naming the filter on its **far** side.
pub const FAR_SIDE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 7;

/// The reading naming where the sun stands, on [`NEAR_SIDE_ID`]'s terms.
pub const SUN_TIME_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 8;

/// The cascade overlay's row.
///
/// **Appended past every id above rather than slotted beside the shadow row it
/// reads next to**, because an id is what a saved selection and every test here
/// name a row by: renumbering to put a new row in the middle would move rows
/// that already exist.
pub const CASCADES_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 9;

/// The atlas viewer's row.
///
/// Appended past every id above, on [`CASCADES_ID`]'s terms and for its reason.
pub const ATLAS_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 10;

/// The reading naming the sun's constant shadow bias — no action; see this
/// module's header.
///
/// Appended past every id above, on [`CASCADES_ID`]'s terms and for its reason.
pub const BIAS_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 11;

/// The reading naming the sun's normal offset, on [`BIAS_ID`]'s terms.
pub const OFFSET_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 12;

/// Every row `ENTER` fires, with the action it carries and the word it prints.
///
/// One table rather than a row list beside an id match, because those are one
/// fact about a row written twice — and the way the two drift is a row that fires
/// its neighbour's action while printing its own name.
pub(crate) const PRESSED_ROWS: [(crcbl::ui::WidgetId, SundialAction, &str); 7] = [
    (
        SHADOWS_ID,
        SundialAction::ToggleEffect(RenderEffects::SHADOWS),
        "SHADOWS",
    ),
    (CASCADES_ID, SundialAction::ToggleCascades, "CASCADES"),
    (ATLAS_ID, SundialAction::ToggleAtlas, "ATLAS"),
    (FILTER_ID, SundialAction::CycleFilter, "FILTER"),
    (SEAM_ID, SundialAction::ToggleSeam, "SEAM"),
    (SUN_ID, SundialAction::ToggleSun, "SUN"),
    (RESET_ID, SundialAction::Reset, "RESET"),
];

/// The action a widget id names, or `None` for a reading and for an id this
/// sample's menus do not use.
#[must_use]
pub fn action_for(id: crcbl::ui::WidgetId) -> Option<SundialAction> {
    if id == CAMERA_ID {
        return Some(SundialAction::CycleCamera);
    }
    PRESSED_ROWS
        .iter()
        .find(|(row, _, _)| *row == id)
        .map(|&(_, action, _)| action)
}

/// What the seam's readings' hint column prints: the pair that moves the seam.
pub const SEAM_KEYS: &str = ", / .";

/// What the constant bias's row prints in its hint column: the pair that walks
/// it.
pub const BIAS_KEYS: &str = "[ / ]";

/// The same for the normal offset's row.
pub const OFFSET_KEYS: &str = "; / '";

/// What the sun reading's hint column prints: the pair that scrubs the clock.
pub const SUN_KEYS: &str = "- / =";

/// What ENTER on the shadow row leaves the request as.
///
/// **Read-modify-write on the programmatic layer, and nothing else** — the layer
/// a panel owns, on `apps/alcove/src/menu.rs`' argument, which is written out
/// there in full. `device` is the fourth layer, which clamps last: a press on a
/// row the device cannot draw therefore changes nothing at all, and the row goes
/// on reading `UNAVAILABLE`.
#[must_use]
pub fn toggled_effect(
    request: EffectRequest,
    device: RenderEffects,
    effect: RenderEffects,
) -> EffectRequest {
    if !device.contains(effect) {
        return request;
    }
    // Against the *resolved* state, so a first press on an effect some other
    // layer had turned off turns it on, rather than forcing off what is already
    // off and looking like a dead row.
    let wanted = !request.resolve(device).contains(effect);
    EffectRequest {
        programmatic: request.programmatic.force(effect, Some(wanted)),
        ..request
    }
}

/// What the shadow row says: the **resolved** answer, and `UNAVAILABLE` rather
/// than `OFF` where the device is what turned it off.
fn effect_state(resolved: RenderEffects, device: RenderEffects) -> &'static str {
    let effect = RenderEffects::SHADOWS;
    if !device.contains(effect) {
        "UNAVAILABLE"
    } else if resolved.contains(effect) {
        "ON"
    } else {
        "OFF"
    }
}

/// What a debug-view row says: `ON` where the frame is drawing `named` and `OFF`
/// otherwise.
///
/// A function rather than the conditional written out at each row, because the
/// two rows are one fact about one cell — the engine draws exactly one view —
/// and a row that spelled the comparison itself is a row that can disagree with
/// its neighbour about what "on" means.
fn on_off(view: DebugView, named: DebugView) -> &'static str {
    if view == named { "ON" } else { "OFF" }
}

/// The pause panel: the camera, the shadow passes, the filter, the seam and the
/// sun.
#[must_use]
pub fn pause_menu(
    camera: CameraMode,
    request: EffectRequest,
    device: RenderEffects,
    knobs: Knobs,
    clock: Clock,
    view: DebugView,
) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    let resolved = request.resolve(device);
    let items = vec![
        MenuItem::new(RESUME_ID, "RESUME", "ESC"),
        MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
        MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
        MenuItem::new(CAMERA_ID, format!("CAMERA: {}", camera.label()), "ENTER"),
        MenuItem::new(
            SHADOWS_ID,
            format!("SHADOWS: {}", effect_state(resolved, device)),
            "ENTER",
        ),
        // Under the shadow row rather than beside the filter's, because it is
        // about the *cascades* and not about the kernel: it says which map a
        // pixel read, which is the question the rows below cannot answer at all.
        //
        // **Both rows are read off one value**, and that is why `view` is a
        // [`DebugView`] rather than a flag each: the engine holds exactly one
        // debug view — `crcbl::debug_view::current` is the cell — so two
        // independent flags here could spell a panel saying `ON` twice about a
        // frame that draws one of them, and two adjacent `bool` arguments could
        // be handed over the wrong way round and still compile.
        MenuItem::new(
            CASCADES_ID,
            format!("CASCADES: {}", on_off(view, DebugView::Cascades)),
            // `ENTER`, not `C`, on `FILTER`'s terms: a row with an action behind
            // it says so, and the key beside it is a shortcut rather than the
            // row's control. This module's header is where that split is drawn.
            "ENTER",
        ),
        // Beside the cascade row: the two are the diagnostics
        // `docs/plan/sample/18-sundial.md`'s milestone 1 asks for, and they are
        // the two rows here that leave the shadow alone and change what is drawn.
        MenuItem::new(
            ATLAS_ID,
            format!("ATLAS: {}", on_off(view, DebugView::ShadowAtlas)),
            "ENTER",
        ),
        MenuItem::new(
            FILTER_ID,
            format!("FILTER: {}", knobs.filter.label()),
            "ENTER",
        ),
        MenuItem::new(SEAM_ID, format!("SEAM: {}", knobs.seam_row()), "ENTER"),
        // The two rows the charter is actually about. Under the seam's own row
        // and in reading order, so they are that row's answer rather than two
        // more switches.
        MenuItem::new(
            NEAR_SIDE_ID,
            format!("NEAR SIDE: {}", knobs.near_side()),
            SEAM_KEYS,
        ),
        MenuItem::new(FAR_SIDE_ID, format!("FAR SIDE: {}", knobs.far_side()), ""),
        // The two counts `docs/plan/sample/18-sundial.md`'s milestone 2 is
        // about, under the seam's readings because they are the other pair a
        // reviewer walks rather than presses. **Readings and not pressed rows**,
        // on this module's header's rule: each moves in two directions, and one
        // `ENTER` cannot say which was meant.
        MenuItem::new(
            BIAS_ID,
            format!("BIAS: {}", Knobs::bias_row(knobs.bias())),
            BIAS_KEYS,
        ),
        MenuItem::new(
            OFFSET_ID,
            format!("NORMAL OFFSET: {}", Knobs::bias_row(knobs.offset())),
            OFFSET_KEYS,
        ),
        MenuItem::new(
            SUN_ID,
            format!(
                "SUN: {}",
                if clock.running() {
                    "RUNNING"
                } else {
                    "STOPPED"
                }
            ),
            "ENTER",
        ),
        MenuItem::new(SUN_TIME_ID, format!("SKY: {}", clock.sky().row()), SUN_KEYS),
        MenuItem::new(RESET_ID, "RESET", "ENTER"),
    ];
    Menu::new("PAUSED", items)
}

/// Sundial's menus, keyed by whether it is paused.
pub type Menus = MenuSet<bool>;

/// The pause menu, not shown.
///
/// `false` — the running fixture — has no entry, which is how the set is told
/// that a running frame draws no menu.
///
/// Built from the values in force at the moment it is called, because
/// [`crcbl::engine::HostedGame::menus`] is a `fn()` with no renderer to ask;
/// `crate::app`'s `menu_kind` replaces the panel with the real ones before the
/// first pause draws it.
#[must_use]
pub fn menus() -> Menus {
    MenuSet::new(
        false,
        vec![(
            true,
            pause_menu(
                CameraMode::default(),
                EffectRequest::default(),
                RenderEffects::all(),
                Knobs::read(),
                Clock::default(),
                crcbl::debug_view::current(),
            ),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun::{FIXTURE_TICK, SCRUB_STEP};

    /// Sundial's whole menu vocabulary: the loop's three, plus its own rows.
    type MenuAction = crcbl::engine::MenuAction<SundialAction>;

    /// Every row's label, in the order the panel draws them.
    fn labels(menu: &Menu) -> Vec<String> {
        menu.items().iter().map(|item| item.label.clone()).collect()
    }

    /// The panel a device that can draw everything shows for `request`.
    fn panel(request: EffectRequest) -> Vec<String> {
        labels(&pause_menu(
            CameraMode::default(),
            request,
            RenderEffects::all(),
            Knobs::read(),
            Clock::default(),
            DebugView::Shaded,
        ))
    }

    /// A running fixture shows no menu, and a paused one shows exactly the pause
    /// menu.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());

        menus.show(true);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");

        menus.show(false);
        assert!(menus.current().is_none());
    }

    /// **Every pressed row carries an action the loop can act on, and no two
    /// carry the same one.**
    ///
    /// The readings are held to the other half of the rule: they carry no action,
    /// and their hint names the **pair** of keys that moves them instead. A
    /// reading wired to an action would be a row whose one `ENTER` had to guess
    /// which of two directions was meant.
    #[test]
    fn every_pressed_row_names_an_action_and_every_reading_names_its_keys() {
        let mut menus = menus();
        menus.show(true);
        let menu = menus.current().expect("the pause menu");

        let readings = [NEAR_SIDE_ID, FAR_SIDE_ID, BIAS_ID, OFFSET_ID, SUN_TIME_ID];
        let mut actions: Vec<MenuAction> = Vec::new();
        for item in menu.items() {
            if readings.contains(&item.id) {
                assert_eq!(
                    action_for(item.id),
                    None,
                    "{} is a reading and must fire nothing",
                    item.label
                );
                continue;
            }
            let action = MenuAction::from_id(item.id, action_for)
                .unwrap_or_else(|| panic!("{} names no action", item.label));
            assert!(
                !actions.contains(&action),
                "the menu carries {action:?} twice",
            );
            actions.push(action);
            assert!(!item.hint.is_empty(), "{} has no key", item.label);
        }

        let rows = labels(menu);
        for (id, action, name) in PRESSED_ROWS {
            assert_eq!(action_for(id), Some(action), "the {name} row");
            assert!(
                actions.contains(&MenuAction::Game(action)),
                "no row fires {action:?}"
            );
            assert!(
                rows.iter().any(|label| label.starts_with(name)),
                "the panel has no {name} row"
            );
        }
        assert!(
            actions.contains(&MenuAction::Game(SundialAction::CycleCamera)),
            "no row moves the camera"
        );
        for (id, keys) in [
            (NEAR_SIDE_ID, SEAM_KEYS),
            (BIAS_ID, BIAS_KEYS),
            (OFFSET_ID, OFFSET_KEYS),
            (SUN_TIME_ID, SUN_KEYS),
        ] {
            let item = menu
                .items()
                .iter()
                .find(|item| item.id == id)
                .expect("the reading is on the panel");
            assert_eq!(item.hint, keys, "{}", item.label);
        }
    }

    /// **The panel prints the value of every knob and the sun's own pose**, which
    /// is the half of milestone 1 the engine could not do: a console variable is
    /// live without being shown, and a tick counter is not shown anywhere at all.
    #[test]
    fn the_panel_prints_every_knob_and_the_suns_pose() {
        let knobs = Knobs::read();
        let clock = Clock::default();
        let rows = labels(&pause_menu(
            CameraMode::default(),
            EffectRequest::default(),
            RenderEffects::all(),
            knobs,
            clock,
            DebugView::Shaded,
        ));
        let has = |prefix: &str| {
            rows.iter()
                .find(|label| label.starts_with(prefix))
                .unwrap_or_else(|| panic!("no {prefix} row in {rows:?}"))
                .clone()
        };
        assert_eq!(has("FILTER: "), format!("FILTER: {}", knobs.filter.label()));
        assert_eq!(has("SEAM: "), format!("SEAM: {}", knobs.seam_row()));
        assert!(has("NEAR SIDE: ").contains(knobs.filter.label()));
        assert!(has("FAR SIDE: ").contains(&knobs.far_side()));
        assert_eq!(has("SUN: "), "SUN: RUNNING");
        assert!(has("SKY: ").contains(&format!("tick {FIXTURE_TICK}")));

        // And a stopped, scrubbed clock reads differently — the check that this
        // panel is built from the values in force rather than from the defaults.
        let mut scrubbed = clock;
        scrubbed.scrub(true);
        let moved = labels(&pause_menu(
            CameraMode::default(),
            EffectRequest::default(),
            RenderEffects::all(),
            knobs,
            scrubbed,
            DebugView::Shaded,
        ));
        let sun = moved
            .iter()
            .find(|label| label.starts_with("SUN: "))
            .expect("a SUN row");
        assert_eq!(sun, "SUN: STOPPED");
        assert!(
            moved
                .iter()
                .any(|label| label.contains(&format!("tick {}", FIXTURE_TICK + SCRUB_STEP))),
            "the panel does not follow the clock: {moved:?}"
        );
    }

    /// The camera row labels the mode it is set to, and the set of modes is a
    /// cycle that comes back round.
    #[test]
    fn the_camera_row_labels_the_mode_it_is_set_to() {
        let camera = |mode| {
            labels(&pause_menu(
                mode,
                EffectRequest::default(),
                RenderEffects::all(),
                Knobs::read(),
                Clock::default(),
                DebugView::Shaded,
            ))
            .into_iter()
            .find(|label| label.starts_with("CAMERA: "))
            .expect("the panel has a camera row")
        };
        assert_eq!(camera(CameraMode::Fixed), "CAMERA: FIXED");
        assert_eq!(camera(CameraMode::Counters), "CAMERA: COUNTERS");
        assert_eq!(camera(CameraMode::Free), "CAMERA: FREE");
        assert_eq!(CameraMode::from_name("fixed"), Some(CameraMode::Fixed));
        assert_eq!(
            CameraMode::from_name("counters"),
            Some(CameraMode::Counters)
        );
        assert_eq!(CameraMode::from_name("free"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("sideways"), None);

        let mut mode = CameraMode::default();
        let mut seen = vec![mode];
        for _ in 0..2 {
            mode = mode.next();
            assert!(!seen.contains(&mode), "the camera cycle repeated {mode:?}");
            seen.push(mode);
        }
        assert_eq!(mode.next(), CameraMode::default(), "the cycle must wrap");
    }

    /// **The shadow row flips the shadow passes and writes only the layer the
    /// menu owns**, and a device that cannot draw them says so rather than
    /// reading as a pass the panel switched off.
    #[test]
    fn the_shadow_row_flips_the_passes_and_writes_only_the_programmatic_layer() {
        let effect = RenderEffects::SHADOWS;
        let device = RenderEffects::all();
        let off = toggled_effect(EffectRequest::default(), device, effect);
        assert_eq!(
            off.resolve(device),
            RenderEffects::DEFAULT_STACK.difference(effect),
            "the row took more than the shadow passes",
        );
        assert!(
            panel(off).contains(&"SHADOWS: OFF".to_string()),
            "{:?}",
            panel(off)
        );

        let back_on = toggled_effect(off, device, effect);
        assert_eq!(back_on.resolve(device), RenderEffects::DEFAULT_STACK);
        assert!(panel(back_on).contains(&"SHADOWS: ON".to_string()));

        // A view whose stack drops the reflections and a player whose quality
        // setting drops the shadows: the row moves the third layer and leaves
        // both of those where they were.
        let request = EffectRequest {
            camera: RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS),
            video: device.difference(effect),
            ..EffectRequest::default()
        };
        let after = toggled_effect(request, device, effect);
        assert_eq!(after.camera, request.camera, "the row rewrote the stack");
        assert_eq!(after.video, request.video, "the row rewrote [engine.video]");
        assert_eq!(
            after.resolve(device),
            RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS),
            "the override must escape the quality clamp, which is what it is for",
        );

        let clamped = RenderEffects::all().difference(effect);
        let rows = labels(&pause_menu(
            CameraMode::default(),
            EffectRequest::default(),
            clamped,
            Knobs::read(),
            Clock::default(),
            DebugView::Shaded,
        ));
        assert!(
            rows.contains(&"SHADOWS: UNAVAILABLE".to_string()),
            "a clamped effect must not read as one the panel switched off: {rows:?}",
        );
        assert_eq!(
            toggled_effect(EffectRequest::default(), clamped, effect),
            EffectRequest::default(),
            "a press on a row the device cannot draw writes nothing",
        );
    }
}
