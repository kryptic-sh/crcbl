//! Alcove's menu: the pause panel, and the rows of its own.
//!
//! Smaller than a game's, for `apps/lantern/src/menu.rs`' reason: this is a
//! fixture, not a game — there is no run to start and nothing to win, so a
//! `GAME OVER` panel would be a screen it could never show.
//!
//! What it does have is every control
//! `docs/plan/sample/19-alcove.md`'s milestones 1 and 2 ask to be *legible*: the
//! technique, the radius, the intensity, the bent-direction switch, the
//! comparison seam — and, because that is the whole of what milestone 2 adds to
//! the engine's half, **which technique each side of the seam is running**.
//!
//! # Two kinds of row, and the difference is deliberate
//!
//! A row with an [`AlcoveAction`] behind it is pressed with `ENTER`, and its
//! hint says so. A row without one is a **reading**: the radius, the intensity
//! and the seam's position each move in two directions, so their control is a
//! *pair* of keys rather than a button, and the hint column names the pair. A
//! press on such a row does nothing, on purpose — one `ENTER` cannot say which
//! of the two directions was meant, and a row that guessed would be a control a
//! reviewer has to hold down nineteen times to undo.
//!
//! `docs/plan/sample/00-samples-overview.md` rule 4 is why there is a panel at
//! all, and the debug overlay's `occlusion` section — [`crate::occlusion::Knobs`]
//! — carries the same readings for a run nobody has paused.

use crcbl::engine::FIRST_GAME_ID;
use crcbl::render::{EffectRequest, RenderEffects};
use crcbl::ui::menu::{Menu, MenuItem, MenuSet};

use crate::occlusion::Knobs;

/// Which camera the frame is drawn from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    /// The pose [`crate::court::fixed_camera`] names, held still.
    ///
    /// The default, and deliberately: a run whose first frame is the golden's
    /// frame is one whose screenshot can be compared to the checked-in reference
    /// without anybody having to stand in the right place first.
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
pub enum AlcoveAction {
    /// Swap the camera, and put the free one back at the golden pose when
    /// switching away from it.
    ToggleCamera,
    /// Flip the occlusion pass itself, in the one layer of
    /// `docs/plan/39-capabilities.md`'s resolution order a panel owns.
    ///
    /// **Not [`ToggleOcclusionView`](Self::ToggleOcclusionView).** This one
    /// removes the passes that compute the channel, which changes what the room
    /// is lit by; that one changes which picture is drawn and leaves the
    /// lighting alone. Standing the two beside each other is also what makes the
    /// second legible — with the pass off, the AO view is white everywhere,
    /// because that is the image the renderer binds when nothing computed a
    /// channel.
    ToggleEffect(RenderEffects),
    /// Swap between the shaded picture and the occlusion channel drawn as grey.
    ToggleOcclusionView,
    /// Move `r_ssao_technique` on to the next gather the engine declares.
    CycleTechnique,
    /// Flip `r_ssao_bent_normals`.
    ToggleBentNormals,
    /// Put the comparison seam up at [`crate::occlusion::SEAM_CENTRE`], or take
    /// it away.
    ToggleSeam,
    /// Put every occlusion knob back to the value the engine declares.
    ResetKnobs,
}

/// The id carrying [`AlcoveAction::ToggleCamera`]. The first id a game may use,
/// per [`FIRST_GAME_ID`].
pub const CAMERA_ID: crcbl::ui::WidgetId = FIRST_GAME_ID;

/// The occlusion pass's own row.
pub const AO_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 1;

/// The AO-only view's row.
pub const AO_VIEW_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 2;

/// The technique selector's row.
pub const TECHNIQUE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 3;

/// The bent-direction switch's row.
pub const BENT_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 4;

/// The comparison seam's row.
pub const SEAM_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 5;

/// The row that puts every knob back.
pub const RESET_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 6;

/// The radius **reading** — no action; see this module's header.
pub const RADIUS_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 7;

/// The intensity reading, on [`RADIUS_ID`]'s terms.
pub const INTENSITY_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 8;

/// The reading naming the technique on the seam's **near** side.
pub const NEAR_SIDE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 9;

/// The reading naming the technique on its **far** side.
pub const FAR_SIDE_ID: crcbl::ui::WidgetId = FIRST_GAME_ID + 10;

/// Every row `ENTER` fires, with the action it carries and the word it prints.
///
/// One table rather than a row list beside an id match, because those are one
/// fact about a row written twice — and the way the two drift is a row that
/// fires its neighbour's action while printing its own name.
pub(crate) const PRESSED_ROWS: [(crcbl::ui::WidgetId, AlcoveAction, &str); 6] = [
    (
        AO_ID,
        AlcoveAction::ToggleEffect(RenderEffects::AMBIENT_OCCLUSION),
        "AO",
    ),
    (AO_VIEW_ID, AlcoveAction::ToggleOcclusionView, "AO VIEW"),
    (TECHNIQUE_ID, AlcoveAction::CycleTechnique, "TECHNIQUE"),
    (BENT_ID, AlcoveAction::ToggleBentNormals, "BENT NORMALS"),
    (SEAM_ID, AlcoveAction::ToggleSeam, "SEAM"),
    (RESET_ID, AlcoveAction::ResetKnobs, "RESET KNOBS"),
];

/// The action a widget id names, or `None` for a reading and for an id this
/// sample's menus do not use.
#[must_use]
pub fn action_for(id: crcbl::ui::WidgetId) -> Option<AlcoveAction> {
    if id == CAMERA_ID {
        return Some(AlcoveAction::ToggleCamera);
    }
    PRESSED_ROWS
        .iter()
        .find(|(row, _, _)| *row == id)
        .map(|&(_, action, _)| action)
}

/// What ENTER on the occlusion row leaves the request as.
///
/// **Read-modify-write on the programmatic layer, and nothing else** — the
/// layer a panel owns, on `apps/lantern/src/menu.rs`' argument, which is written
/// out there in full. `device` is the fourth layer, which clamps last: a press
/// on a row the device cannot draw therefore changes nothing at all, and the row
/// goes on reading `UNAVAILABLE`.
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

/// What the occlusion row says: the **resolved** answer, and `UNAVAILABLE`
/// rather than `OFF` where the device is what turned it off.
fn effect_state(resolved: RenderEffects, device: RenderEffects) -> &'static str {
    let effect = RenderEffects::AMBIENT_OCCLUSION;
    if !device.contains(effect) {
        "UNAVAILABLE"
    } else if resolved.contains(effect) {
        "ON"
    } else {
        "OFF"
    }
}

/// The pause panel: the camera, the occlusion pass, and every knob
/// `docs/plan/sample/19-alcove.md` asks to be shown.
#[must_use]
pub fn pause_menu(
    camera: CameraMode,
    request: EffectRequest,
    device: RenderEffects,
    knobs: Knobs,
) -> Menu {
    use crcbl::engine::{DEBUG_OVERLAY_ID, FULLSCREEN_ID, RESUME_ID};
    let resolved = request.resolve(device);
    let on_off = |on: bool| if on { "ON" } else { "OFF" };
    let items = vec![
        MenuItem::new(RESUME_ID, "RESUME", "ESC"),
        MenuItem::new(FULLSCREEN_ID, "FULLSCREEN", "F11"),
        MenuItem::new(DEBUG_OVERLAY_ID, "DEBUG PANEL", "F3"),
        MenuItem::new(CAMERA_ID, format!("CAMERA: {}", camera.label()), "ENTER"),
        MenuItem::new(
            AO_ID,
            format!("AO: {}", effect_state(resolved, device)),
            "ENTER",
        ),
        MenuItem::new(
            AO_VIEW_ID,
            format!("AO VIEW: {}", on_off(knobs.occlusion_view)),
            "ENTER",
        ),
        MenuItem::new(
            TECHNIQUE_ID,
            format!("TECHNIQUE: {}", knobs.technique),
            "ENTER",
        ),
        MenuItem::new(
            BENT_ID,
            format!("BENT NORMALS: {}", on_off(knobs.bent_normals)),
            "ENTER",
        ),
        MenuItem::new(
            RADIUS_ID,
            format!("RADIUS: {:.3} m", knobs.radius),
            RADIUS_KEYS,
        ),
        MenuItem::new(
            INTENSITY_ID,
            format!("INTENSITY: {:.2}", knobs.intensity),
            INTENSITY_KEYS,
        ),
        MenuItem::new(SEAM_ID, format!("SEAM: {}", knobs.seam_row()), "ENTER"),
        // The two rows milestone 2 is actually about. Under the seam's own row
        // and in reading order, so they are that row's answer rather than two
        // more switches.
        MenuItem::new(
            NEAR_SIDE_ID,
            format!("NEAR SIDE: {}", knobs.near_side()),
            SEAM_KEYS,
        ),
        MenuItem::new(FAR_SIDE_ID, format!("FAR SIDE: {}", knobs.far_side()), ""),
        MenuItem::new(RESET_ID, "RESET KNOBS", "ENTER"),
    ];
    Menu::new("PAUSED", items)
}

/// What the radius row's hint column prints.
pub const RADIUS_KEYS: &str = "[ / ]";

/// What the intensity row's hint column prints.
pub const INTENSITY_KEYS: &str = "- / =";

/// What the near-side row's hint column prints: the pair that moves the seam.
pub const SEAM_KEYS: &str = ", / .";

/// Alcove's menus, keyed by whether it is paused.
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
            ),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alcove's whole menu vocabulary: the loop's three, plus its own rows.
    type MenuAction = crcbl::engine::MenuAction<AlcoveAction>;

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
        ))
    }

    /// A running fixture shows no menu, and a paused one shows exactly the
    /// pause menu.
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
    /// The readings are held to the other half of the rule: they carry no
    /// action, and their hint names the **pair** of keys that moves them
    /// instead. A reading wired to an action would be a row whose one `ENTER`
    /// had to guess which of two directions was meant.
    #[test]
    fn every_pressed_row_names_an_action_and_every_reading_names_its_keys() {
        let mut menus = menus();
        menus.show(true);
        let menu = menus.current().expect("the pause menu");

        let readings = [RADIUS_ID, INTENSITY_ID, NEAR_SIDE_ID, FAR_SIDE_ID];
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

        for (id, action, name) in PRESSED_ROWS {
            assert_eq!(action_for(id), Some(action), "the {name} row");
            assert!(
                actions.contains(&MenuAction::Game(action)),
                "no row fires {action:?}"
            );
            assert!(
                panel(EffectRequest::default())
                    .iter()
                    .any(|label| label.starts_with(name)),
                "the panel has no {name} row"
            );
        }
        assert!(
            actions.contains(&MenuAction::Game(AlcoveAction::ToggleCamera)),
            "no row swaps the camera"
        );
        // Each reading's hint is a key **pair**, which is what says it is held
        // rather than pressed — except the far side, which nothing moves.
        for (id, keys) in [
            (RADIUS_ID, RADIUS_KEYS),
            (INTENSITY_ID, INTENSITY_KEYS),
            (NEAR_SIDE_ID, SEAM_KEYS),
        ] {
            let item = menu
                .items()
                .iter()
                .find(|item| item.id == id)
                .expect("the reading is on the panel");
            assert_eq!(item.hint, keys, "{}", item.label);
        }
    }

    /// **The panel prints the value of every knob**, which is the half of
    /// milestone 1 the engine could not do: a console variable is live without
    /// being shown.
    #[test]
    fn the_panel_prints_the_value_of_every_knob() {
        let knobs = Knobs::read();
        let rows = labels(&pause_menu(
            CameraMode::default(),
            EffectRequest::default(),
            RenderEffects::all(),
            knobs,
        ));
        let has = |prefix: &str| {
            rows.iter()
                .find(|label| label.starts_with(prefix))
                .unwrap_or_else(|| panic!("no {prefix} row in {rows:?}"))
                .clone()
        };
        assert_eq!(has("RADIUS: "), format!("RADIUS: {:.3} m", knobs.radius));
        assert_eq!(
            has("INTENSITY: "),
            format!("INTENSITY: {:.2}", knobs.intensity)
        );
        assert_eq!(
            has("TECHNIQUE: "),
            format!("TECHNIQUE: {}", knobs.technique)
        );
        assert_eq!(has("SEAM: "), format!("SEAM: {}", knobs.seam_row()));
        assert!(has("NEAR SIDE: ").contains(knobs.technique));
        assert!(has("FAR SIDE: ").contains(&knobs.far_side()));
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
                Knobs::read(),
            ))
            .into_iter()
            .find(|label| label.starts_with("CAMERA: "))
            .expect("the panel has a camera row")
        };
        assert_eq!(camera(CameraMode::Fixed), "CAMERA: FIXED");
        assert_eq!(camera(CameraMode::Free), "CAMERA: FREE");
        assert_eq!(CameraMode::from_name("fixed"), Some(CameraMode::Fixed));
        assert_eq!(CameraMode::from_name("free"), Some(CameraMode::Free));
        assert_eq!(CameraMode::from_name("sideways"), None);
        assert_eq!(CameraMode::default().toggled(), CameraMode::Free);
        assert_eq!(CameraMode::default().toggled().toggled(), CameraMode::Fixed);
    }

    /// **The AO row flips the occlusion pass and writes only the layer the menu
    /// owns**, and an effect the device cannot draw reads as unavailable rather
    /// than as one the panel switched off.
    #[test]
    fn the_ao_row_flips_the_pass_and_writes_only_the_programmatic_layer() {
        let effect = RenderEffects::AMBIENT_OCCLUSION;
        let device = RenderEffects::all();
        let off = toggled_effect(EffectRequest::default(), device, effect);
        assert_eq!(
            off.resolve(device),
            RenderEffects::DEFAULT_STACK.difference(effect),
            "the row took more than the occlusion pass",
        );
        assert!(
            panel(off).contains(&"AO: OFF".to_string()),
            "{:?}",
            panel(off)
        );

        let back_on = toggled_effect(off, device, effect);
        assert_eq!(back_on.resolve(device), RenderEffects::DEFAULT_STACK);
        assert!(panel(back_on).contains(&"AO: ON".to_string()));

        // A view whose stack drops the reflections and a player whose quality
        // setting drops the occlusion: the row must move the third layer and
        // leave both of those where they were.
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
        ));
        assert!(
            rows.contains(&"AO: UNAVAILABLE".to_string()),
            "a clamped effect must not read as one the panel switched off: {rows:?}",
        );
        assert_eq!(
            toggled_effect(EffectRequest::default(), clamped, effect),
            EffectRequest::default(),
            "a press on a row the device cannot draw writes nothing",
        );
    }
}
