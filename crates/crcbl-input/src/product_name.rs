//! What a pad's product name says about it: its [`PadKind`].
//!
//! Shared by the backends that learn a name and no USB ids — `web_gamepad`,
//! for a `Gamepad.id` that carries none, and `game_controller`, whose
//! `productCategory` and `vendorName` are all GameController tells — so the
//! two cannot disagree about which family a name belongs to.

use crate::PadKind;

/// A pad's family from its product name, matched without regard to case
/// against the families' own names. Anything else is [`PadKind::Generic`].
///
/// GameController's `productCategory` constants — `"DualShock 4"`,
/// `"DualSense"`, `"Xbox One"`, `"Switch Pro Controller"`, `"Nintendo Switch
/// Joy-Con (L/R)"` — each land on their family here too, which the macOS
/// smoke test in `game_controller` checks against the framework's own
/// constants.
pub(crate) fn kind_of_name(name: &str) -> PadKind {
    let name = name.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| name.contains(needle));
    if has(&["xinput", "xbox"]) {
        PadKind::Xbox
    } else if has(&["dualsense", "dualshock", "playstation"]) {
        PadKind::PlayStation
    } else if has(&["pro controller", "joy-con"]) {
        PadKind::Switch
    } else if has(&["steam deck"]) {
        PadKind::SteamDeck
    } else {
        PadKind::Generic
    }
}
