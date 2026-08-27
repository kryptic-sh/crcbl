//! Lantern's menu: the pause panel, and the rows of its own.
//!
//! Smaller than a game's, for `apps/sandbox/src/menu.rs`'s reason: this is a
//! fixture, not a game — there is no run to start, no score to lose and nothing
//! to win, so a `GAME OVER` panel would be a screen it could never show.
//!
//! The first row it does have is the camera. A lighting fixture is looked at
//! from two places — wherever the reviewer walked to, and the pose the golden
//! was taken from — and getting back to the second is a thing a reviewer wants
//! to do constantly. `docs/plan/sample/00-samples-overview.md` rule 4 is why
//! there is a panel at all, and it is the same reason there is this: a sample
//! that cannot show the engine's menu is a finding about the menu.
//!
//! The rest are `docs/plan/18-render-features.md`'s effects, one row each, which
//! is the charter's milestone 4: the three switches `--no-shadows`, `--no-ao`
//! and `--no-reflections` set, reachable with the room on screen. Holding a lit
//! room against an unlit one is then a keypress rather than a restart, and the
//! two vocabularies are one — the row for `--no-ao` is spelled `AO`.
//!
//! # A row writes one layer and reads the answer of all four
//!
//! `docs/plan/39-capabilities.md`'s order resolves four layers, and a panel owns
//! exactly one of them — [`toggled_effect`] says which and why. What a row
//! *shows* comes from the other end of the same order: [`EffectRequest::resolve`]
//! against what the device permits, so a row cannot tick an effect the hardware
//! clamped away, and an effect no device offers reads as unavailable rather than
//! as a switch that does nothing.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::render::{EffectRequest, RenderEffects};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The pose `crate::room::fixed_camera` names, held still.
    ///
    /// The default, and deliberately: a run whose first frame is the golden's
    /// frame is one whose screenshot can be compared to the checked-in
    /// reference without anybody having to stand in the right place first.
    #[default]
    Fixed,
    /// [`crcbl::render::Flyer`], starting at that same pose.
    Free,
}

impl CameraMode {
    /// Parses `fixed` / `free`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "fixed" | "golden" => Some(Self::Fixed),
            "free" | "fly" | "free-fly" => Some(Self::Free),
            _ => None,
        }
    }

    /// The other one.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Fixed => Self::Free,
            Self::Free => Self::Fixed,
        }
    }

    /// What the panel and the summary call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fixed => "FIXED",
            Self::Free => "FREE",
        }
    }
}

/// The actions this sample's menus have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanternAction {
    /// Swap the camera, and put the free one back at the golden pose when
    /// switching away from it.
    ToggleCamera,
    /// Flip one of topic 18's effects, in the one layer of the resolution order
    /// a panel owns — see [`toggled_effect`].
    ///
    /// Always a single flag: the only source of these is the row table, and a
    /// several-effect action would have no one state to flip.
    ToggleEffect(RenderEffects),
    /// Swap between the shaded picture and the ambient-occlusion channel drawn
    /// as grey — `crcbl_render::ForwardRenderer::set_occlusion_view`.
    ///
    /// **Not a [`ToggleEffect`](Self::ToggleEffect)**, and the distinction is
    /// the point of the row: the `AO` row above turns the occlusion pass off,
    /// which changes the picture the room is lit by, while this one changes
    /// which picture is drawn and leaves the lighting alone. Standing the two
    /// beside each other is also what makes the second legible — with `AO` off
    /// this view is white everywhere, because that is the image the renderer
    /// binds when nothing computed a channel.
    ToggleOcclusionView,
}

/// The id carrying [`LanternAction::ToggleCamera`]. The first id a game may use,
/// per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The id carrying the shadow atlas's [`LanternAction::ToggleEffect`].
pub const SHADOWS_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The ambient-occlusion row's, on [`SHADOWS_ID`]'s terms.
pub const AO_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 2;

/// The screen-space reflection row's, on [`SHADOWS_ID`]'s terms.
pub const REFLECTIONS_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 3;

/// The id carrying [`LanternAction::ToggleOcclusionView`].
///
/// Outside [`EFFECT_ROWS`] because it is not an effect: that table pairs an id
/// with a [`RenderEffects`] bit, and this row has no bit to pair with.
pub const AO_VIEW_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 4;

/// The effect rows, in the order the panel draws them: the id one fires, the
/// bit it flips, and the word it prints.
///
/// One table rather than a row list beside an id match, because those are one
/// fact about a row written twice — and the way the two drift is a row that
/// fires its neighbour's effect while printing its own name.
///
/// The names are the command line's: `--no-ao` and the `AO` row are visibly the
/// same switch.
pub(crate) const EFFECT_ROWS: [(crcbl::ui::WidgetId, RenderEffects, &str); 3] = [
    (SHADOWS_ID, RenderEffects::SHADOWS, "SHADOWS"),
    (AO_ID, RenderEffects::AMBIENT_OCCLUSION, "AO"),
    (REFLECTIONS_ID, RenderEffects::REFLECTIONS, "REFLECTIONS"),
];

/// The action a widget id names, or `None` for an id this game's menus do not
/// use.
#[must_use]
pub fn action_for(id: crcbl::ui::WidgetId) -> Option<LanternAction> {
    if id == CAMERA_ID {
        return Some(LanternAction::ToggleCamera);
    }
    if id == AO_VIEW_ID {
        return Some(LanternAction::ToggleOcclusionView);
    }
    EFFECT_ROWS
        .iter()
        .find(|(row, _, _)| *row == id)
        .map(|&(_, effect, _)| LanternAction::ToggleEffect(effect))
}

/// What ENTER on `effect`'s row leaves the request as.
///
/// **Read-modify-write on the programmatic layer, and nothing else.** That is
/// the layer a panel owns: the camera stack is a view's own answer and
/// `[engine.video]` is the player's saved quality setting, so a row that rewrote
/// either would throw away a decision it was never asked about — and a row that
/// replaced the whole request would throw away both. It is also the only layer
/// that can move a decision *upward*, which a toggle needs: turning shadows back
/// on after `--no-shadows` means escaping the layer that flag wrote.
///
/// `device` is the fourth layer, which clamps last and cannot be overridden
/// upward. A press on a row the device cannot draw therefore changes nothing at
/// all — writing an override no frame could honour would leave the request
/// claiming something the picture never does — and the row goes on reading
/// `UNAVAILABLE`.
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

/// What an effect row says about one effect.
///
/// The **resolved** answer rather than the requested one, and `UNAVAILABLE`
/// rather than `OFF` where the device is what turned it off: a row ticking a
/// request the device clamped away would be a switch that does nothing, which is
/// the defect `docs/plan/18-render-features.md` records against the first AO
/// off-switch.
fn effect_state(
    effect: RenderEffects,
    resolved: RenderEffects,
    device: RenderEffects,
) -> &'static str {
    if !device.contains(effect) {
        "UNAVAILABLE"
    } else if resolved.contains(effect) {
        "ON"
    } else {
        "OFF"
    }
}

/// The pause panel: the camera row labelled with the mode in force, and one row
/// per effect labelled with what the frame draws.
///
/// `request` is the three layers a caller supplies and `device` the fourth, so
/// the labels are `request.resolve(device)` — see this module's second section.
#[must_use]
pub fn pause_menu(
    camera: CameraMode,
    request: EffectRequest,
    device: RenderEffects,
    occlusion_view: bool,
) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    let resolved = request.resolve(device);
    let mut items = vec![
        MenuItem::new(RESUME_ID, "RESUME", "ESC"),
        MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
        MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
        MenuItem::new(CAMERA_ID, format!("CAMERA: {}", camera.label()), "ENTER"),
    ];
    items.extend(EFFECT_ROWS.iter().map(|&(id, effect, name)| {
        let state = effect_state(effect, resolved, device);
        MenuItem::new(id, format!("{name}: {state}"), "ENTER")
    }));
    // Last, and under the effect rows on purpose: it is the row that says what
    // the `AO` row above it is doing, so it reads as that row's companion rather
    // than as a fourth effect.
    items.push(MenuItem::new(
        AO_VIEW_ID,
        format!("AO VIEW: {}", if occlusion_view { "ON" } else { "OFF" }),
        "ENTER",
    ));
    Menu::new("PAUSED", items)
}

/// Lantern's menus, keyed by whether it is paused.
pub type Menus = MenuSet<bool>;

/// The pause menu, not shown.
///
/// `false` — the running fixture — has no entry, which is how the set is told
/// that a running frame draws no menu.
///
/// Built from the defaults — the golden pose, every effect, a device that
/// permits them all — because the values in force live on the renderer and this
/// is called before there is one. `crate::app`'s `menu_kind` replaces the panel
/// with them before the first pause draws it.
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
                false,
            ),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lantern's whole menu vocabulary: the loop's three, plus its own rows.
    type MenuAction = crcbl::engine::MenuAction<LanternAction>;

    /// The action the highlighted button carries, which is what the loop reads.
    fn activate(menus: &mut Menus) -> Option<MenuAction> {
        menus
            .activate()
            .and_then(|id| MenuAction::from_id(id, action_for))
    }

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
            false,
        ))
    }

    /// A running fixture shows no menu, and a paused one shows exactly the
    /// pause menu.
    #[test]
    fn the_menu_is_shown_only_while_paused() {
        let mut menus = menus();
        assert!(menus.current().is_none());
        assert_eq!(activate(&mut menus), None, "nothing to fire");

        menus.show(true);
        assert_eq!(menus.current().expect("the pause menu").title, "PAUSED");

        menus.show(false);
        assert!(menus.current().is_none());
    }

    /// Every button carries an action the loop can act on, no two carry the
    /// same one, and each prints the key that does the same thing.
    #[test]
    fn every_button_names_an_action_the_loop_handles() {
        let mut menus = menus();
        menus.show(true);
        let menu = menus.current().expect("the pause menu");
        let actions: Vec<MenuAction> = menu
            .items()
            .iter()
            .map(|item| {
                MenuAction::from_id(item.id, action_for)
                    .unwrap_or_else(|| panic!("{} names no action", item.label))
            })
            .collect();
        for (index, action) in actions.iter().enumerate() {
            assert!(
                !actions[..index].contains(action),
                "the menu carries {action:?} twice",
            );
        }
        for item in menu.items() {
            assert!(!item.hint.is_empty(), "{} has no key", item.label);
        }
        for expected in [
            MenuAction::Game(LanternAction::ToggleCamera),
            MenuAction::Game(LanternAction::ToggleEffect(RenderEffects::SHADOWS)),
            MenuAction::Game(LanternAction::ToggleEffect(
                RenderEffects::AMBIENT_OCCLUSION,
            )),
            MenuAction::Game(LanternAction::ToggleEffect(RenderEffects::REFLECTIONS)),
        ] {
            assert!(actions.contains(&expected), "no row fires {expected:?}");
        }
    }

    /// **The AO-view row is a view switch, not a fourth effect.**
    ///
    /// Two halves, and the second is the one worth guarding: it labels its own
    /// state like every other row, and pressing it fires an action that is not
    /// a [`LanternAction::ToggleEffect`]. Wired to a `ToggleEffect` instead it
    /// would read plausibly on the panel and turn the occlusion *pass* off,
    /// which is the row above it.
    #[test]
    fn the_ambient_occlusion_view_row_switches_the_view_not_the_pass() {
        let rows = |on| {
            labels(&pause_menu(
                CameraMode::default(),
                EffectRequest::default(),
                RenderEffects::all(),
                on,
            ))
        };
        assert!(
            rows(false).contains(&"AO VIEW: OFF".to_string()),
            "{:?}",
            rows(false)
        );
        assert!(
            rows(true).contains(&"AO VIEW: ON".to_string()),
            "{:?}",
            rows(true)
        );
        assert_eq!(
            action_for(AO_VIEW_ID),
            Some(LanternAction::ToggleOcclusionView),
        );
        assert!(
            !EFFECT_ROWS.iter().any(|&(id, _, _)| id == AO_VIEW_ID),
            "the view row must not sit in the effect table",
        );
    }

    /// The camera row labels the mode it is set to, so a reviewer can read it
    /// off the panel rather than guessing from the picture.
    #[test]
    fn the_camera_row_labels_the_mode_it_is_set_to() {
        let camera = |mode| {
            labels(&pause_menu(
                mode,
                EffectRequest::default(),
                RenderEffects::all(),
                false,
            ))
            .into_iter()
            .find(|label| label.starts_with("CAMERA: "))
            .expect("the panel has a camera row")
        };
        assert_eq!(camera(CameraMode::Fixed), "CAMERA: FIXED");
        assert_eq!(camera(CameraMode::Free), "CAMERA: FREE");
    }

    /// **A row flips its own effect, leaves the other two where they were, and
    /// says which it did.**
    ///
    /// One arm per row rather than one assertion over all three, for
    /// `crate::args`' reason: the failure worth catching is a row wired to the
    /// wrong bit, and a test that pressed every row would report the empty set
    /// whichever bit each one flipped.
    ///
    /// The three rows are written out here rather than read from the table the
    /// panel is built from: a loop over that table asks whether it agrees with
    /// itself, which a row firing its neighbour's effect passes.
    #[test]
    fn an_effect_row_flips_its_own_effect_and_leaves_the_others_alone() {
        let device = RenderEffects::all();
        for (id, effect, name) in [
            (SHADOWS_ID, RenderEffects::SHADOWS, "SHADOWS"),
            (AO_ID, RenderEffects::AMBIENT_OCCLUSION, "AO"),
            (REFLECTIONS_ID, RenderEffects::REFLECTIONS, "REFLECTIONS"),
        ] {
            let Some(LanternAction::ToggleEffect(fired)) = action_for(id) else {
                panic!("the {name} row fires no effect");
            };
            assert_eq!(fired, effect, "the {name} row fires the wrong effect");

            let off = toggled_effect(EffectRequest::default(), device, fired);
            // `DEFAULT_STACK` and not `device`: the request's camera layer is
            // the default one, which asks for every effect that models the
            // scene's own light transport and for no lens effect. What this arm
            // is about is the one bit the row moved.
            assert_eq!(
                off.resolve(device),
                RenderEffects::DEFAULT_STACK.difference(effect),
                "the {name} row took more than its own effect",
            );
            assert!(
                panel(off).contains(&format!("{name}: OFF")),
                "the {name} row does not say it is off: {:?}",
                panel(off),
            );

            let back_on = toggled_effect(off, device, fired);
            assert_eq!(
                back_on.resolve(device),
                RenderEffects::DEFAULT_STACK,
                "{name} did not come back"
            );
            assert!(panel(back_on).contains(&format!("{name}: ON")), "{name}");
        }

        // And they compose, which is the state `--no-shadows --no-ao` starts a
        // run in and a reviewer reaches with two presses.
        let shadows = toggled_effect(EffectRequest::default(), device, RenderEffects::SHADOWS);
        let and_ao = toggled_effect(shadows, device, RenderEffects::AMBIENT_OCCLUSION);
        assert_eq!(
            and_ao.resolve(device),
            RenderEffects::DEFAULT_STACK
                .difference(RenderEffects::SHADOWS)
                .difference(RenderEffects::AMBIENT_OCCLUSION)
        );
    }

    /// **A row writes the programmatic layer and no other**, and the override it
    /// writes escapes the quality clamp rather than rewriting it.
    ///
    /// The failure this catches is a row that hands the renderer a whole fresh
    /// [`EffectRequest`]: the frame it draws is right, and a camera stack or an
    /// `[engine.video]` decision the panel was never asked about is gone. Both
    /// layers are unwired today, so nothing else in this tree would notice.
    #[test]
    fn an_effect_row_writes_only_the_layer_the_menu_owns() {
        let device = RenderEffects::all();
        // A view with no reflections in its stack, and a player whose quality
        // setting turned occlusion off.
        let request = EffectRequest {
            camera: RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS),
            video: device.difference(RenderEffects::AMBIENT_OCCLUSION),
            ..EffectRequest::default()
        };
        assert_eq!(
            request.resolve(device),
            RenderEffects::DEFAULT_STACK
                .difference(RenderEffects::REFLECTIONS)
                .difference(RenderEffects::AMBIENT_OCCLUSION),
            "the two clamps leave the shadows and the resolve and nothing else",
        );
        assert!(
            panel(request).contains(&"AO: OFF".to_string()),
            "an effect another layer turned off is off, not unavailable: {:?}",
            panel(request),
        );

        let after = toggled_effect(request, device, RenderEffects::AMBIENT_OCCLUSION);
        assert_eq!(after.camera, request.camera, "the row rewrote the stack");
        assert_eq!(after.video, request.video, "the row rewrote [engine.video]");
        assert_eq!(
            after.resolve(device),
            RenderEffects::DEFAULT_STACK.difference(RenderEffects::REFLECTIONS),
            "the override must escape the quality clamp, which is what it is for",
        );
    }

    /// **An effect the device cannot draw reads as unavailable, not as off**,
    /// and pressing its row changes nothing.
    ///
    /// The two are one missing bit in the resolved set and a reviewer has to be
    /// able to tell them apart: a row showing `OFF` for a clamped effect is a
    /// switch that does nothing, which is the AO off-switch defect
    /// `docs/plan/18-render-features.md` records.
    #[test]
    fn a_row_the_device_cannot_draw_reads_as_unavailable() {
        let device = RenderEffects::all().difference(RenderEffects::REFLECTIONS);
        let request = toggled_effect(EffectRequest::default(), device, RenderEffects::SHADOWS);
        let rows = labels(&pause_menu(CameraMode::default(), request, device, false));
        assert!(rows.contains(&"SHADOWS: OFF".to_string()), "{rows:?}");
        assert!(rows.contains(&"AO: ON".to_string()), "{rows:?}");
        assert!(
            rows.contains(&"REFLECTIONS: UNAVAILABLE".to_string()),
            "a clamped effect must not read as one the panel switched off: {rows:?}",
        );

        let pressed = toggled_effect(request, device, RenderEffects::REFLECTIONS);
        assert_eq!(
            pressed, request,
            "the press wrote an override the device clamp discards",
        );
        assert_eq!(
            pressed.resolve(device),
            RenderEffects::DEFAULT_STACK
                .difference(RenderEffects::SHADOWS)
                .difference(RenderEffects::REFLECTIONS),
            "and it must not have moved the frame either",
        );
    }

    /// The two spellings of each mode round-trip, and nothing else parses.
    #[test]
    fn the_camera_modes_parse_by_name() {
        assert_eq!(CameraMode::from_name("fixed"), Some(CameraMode::Fixed));
        assert_eq!(CameraMode::from_name("free"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("sideways"), None);
        assert_eq!(CameraMode::default().toggled(), CameraMode::Free);
        assert_eq!(CameraMode::default().toggled().toggled(), CameraMode::Fixed);
    }
}
