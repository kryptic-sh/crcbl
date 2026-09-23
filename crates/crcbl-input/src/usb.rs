//! What a pad's USB vendor and product ids say about it: its [`PadKind`].
//!
//! Shared by the backends that learn those ids — `evdev` from `EVIOCGID`, and
//! `web_gamepad` from the ids a browser writes into `Gamepad.id` — so the two
//! cannot disagree about which family a controller belongs to.

use crate::PadKind;

/// Microsoft's USB vendor id.
pub(crate) const VENDOR_MICROSOFT: u16 = 0x045e;
/// Sony's.
pub(crate) const VENDOR_SONY: u16 = 0x054c;
/// Nintendo's.
pub(crate) const VENDOR_NINTENDO: u16 = 0x057e;
/// Valve's: the Deck's built-in controls, and Steam Input's virtual pad.
pub(crate) const VENDOR_VALVE: u16 = 0x28de;
/// The Steam Deck's built-in controller (`USB_DEVICE_ID_STEAM_DECK`).
pub(crate) const PRODUCT_STEAM_DECK: u16 = 0x1205;

/// A device's family, from its vendor and product ids.
///
/// Valve's other products are Steam Input's virtual pad, which presents
/// itself as an Xbox 360 controller, and the Steam Controller; both are laid
/// out like an Xbox pad.
pub(crate) const fn kind_of(vendor: u16, product: u16) -> PadKind {
    match (vendor, product) {
        (VENDOR_VALVE, PRODUCT_STEAM_DECK) => PadKind::SteamDeck,
        (VENDOR_MICROSOFT | VENDOR_VALVE, _) => PadKind::Xbox,
        (VENDOR_SONY, _) => PadKind::PlayStation,
        (VENDOR_NINTENDO, _) => PadKind::Switch,
        _ => PadKind::Generic,
    }
}
