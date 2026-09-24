//! `ISteamUtils`: facts about the app and the machine.

use crate::{AppId, Steam};

/// Which Steam hardware the game is running on
/// (`ISteamUtils::IsRunningOnSteamHardware`, `ESteamHardwareType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SteamHardware {
    /// Not Steam hardware (`k_ESteamHardwareType_None`).
    None,
    /// A Steam Deck (`k_ESteamHardwareType_SteamDeck`).
    SteamDeck,
    /// A Steam Machine (`k_ESteamHardwareType_SteamMachine`).
    SteamMachine,
    /// A Steam Frame (`k_ESteamHardwareType_SteamFrame`).
    SteamFrame,
    /// A value this crate does not name. Valve's own comment on
    /// `IsRunningOnSteamHardware` warns that future hardware will return
    /// values older SDKs do not know, so an unnamed value is kept rather than
    /// mistaken for [`None`](Self::None).
    Unknown(i32),
}

impl SteamHardware {
    /// Maps an `ESteamHardwareType`.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::SteamDeck,
            2 => Self::SteamMachine,
            3 => Self::SteamFrame,
            other => Self::Unknown(other),
        }
    }
}

/// The settings preset Steam suggests for this machine
/// (`ISteamUtils::GetSteamHardwareDefaultConfig`,
/// `ESteamHardwareDefaultConfig`).
///
/// Valve's intended use, from the header: map a machine-specific value to a
/// configuration tuned for that device; map a general one to the matching
/// user preset; fall back to the game's own heuristics for anything else. The
/// partner site can change what a given device answers after release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardwareDefaultConfig {
    /// No suggestion (`k_ESteamHardwareDefaultConfigNone`).
    None,
    /// The game's low preset (`k_ESteamHardwareDefaultConfigLow`).
    Low,
    /// The game's medium preset (`k_ESteamHardwareDefaultConfigMedium`).
    Medium,
    /// The game's high preset (`k_ESteamHardwareDefaultConfigHigh`).
    High,
    /// The game's highest preset (`k_ESteamHardwareDefaultConfigMax`).
    Max,
    /// A configuration tuned for the Steam Deck, or hardware that performs
    /// like one (`k_ESteamHardwareDefaultConfigSteamDeck`).
    SteamDeck,
    /// A configuration tuned for the Steam Machine
    /// (`k_ESteamHardwareDefaultConfigSteamMachine`).
    SteamMachine,
    /// A configuration tuned for the Steam Frame
    /// (`k_ESteamHardwareDefaultConfigSteamFrame`).
    SteamFrame,
    /// A value this crate does not name — future hardware, as with
    /// [`SteamHardware::Unknown`].
    Unknown(i32),
}

impl HardwareDefaultConfig {
    /// Maps an `ESteamHardwareDefaultConfig`.
    #[must_use]
    pub const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::Low,
            2 => Self::Medium,
            3 => Self::High,
            4 => Self::Max,
            5 => Self::SteamDeck,
            6 => Self::SteamMachine,
            7 => Self::SteamFrame,
            other => Self::Unknown(other),
        }
    }
}

/// The screen corner Steam's overlay notifications pop up in
/// (`ENotificationPosition`, without its `k_EPositionInvalid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationCorner {
    /// `k_EPositionTopLeft`.
    TopLeft,
    /// `k_EPositionTopRight`.
    TopRight,
    /// `k_EPositionBottomLeft`.
    BottomLeft,
    /// `k_EPositionBottomRight` — Steam's default.
    BottomRight,
}

impl NotificationCorner {
    /// The `ENotificationPosition` value.
    const fn raw(self) -> i32 {
        match self {
            Self::TopLeft => 0,
            Self::TopRight => 1,
            Self::BottomLeft => 2,
            Self::BottomRight => 3,
        }
    }
}

/// `ISteamUtils`, borrowed from a [`Steam`]; from [`Steam::utils`].
#[derive(Debug, Clone, Copy)]
pub struct Utils<'a> {
    pub(crate) steam: &'a Steam,
}

impl Steam {
    /// Facts about the app and the machine.
    #[must_use]
    pub fn utils(&self) -> Utils<'_> {
        Utils { steam: self }
    }
}

impl Utils<'_> {
    /// The app id Steam is running this process as (`ISteamUtils::GetAppID`).
    #[must_use]
    pub fn app_id(&self) -> AppId {
        let client = &self.steam.client;
        // SAFETY: `client.utils` is the non-null `ISteamUtils` init resolved,
        // and Steam stays initialised while `client` lives. Every call below
        // relies on the same.
        AppId(unsafe { (client.lib.fns.utils.get_app_id)(client.utils) })
    }

    /// Which Steam hardware this is.
    #[must_use]
    pub fn steam_hardware(&self) -> SteamHardware {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        SteamHardware::from_raw(unsafe {
            (client.lib.fns.utils.is_running_on_steam_hardware)(client.utils)
        })
    }

    /// The settings preset Steam suggests for this machine.
    #[must_use]
    pub fn hardware_default_config(&self) -> HardwareDefaultConfig {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        HardwareDefaultConfig::from_raw(unsafe {
            (client.lib.fns.utils.get_steam_hardware_default_config)(client.utils)
        })
    }

    /// Whether the game is running under Proton, Valve's Windows
    /// compatibility layer (`ISteamUtils::IsRunningUnderProton`).
    #[must_use]
    pub fn under_proton(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        unsafe { (client.lib.fns.utils.is_running_under_proton)(client.utils) }
    }

    /// Whether the Steam overlay is enabled and ready for this process
    /// (`ISteamUtils::IsOverlayEnabled`). It can take a few seconds after
    /// init to become true, and never does when Steam did not inject the
    /// overlay — see `docs/notes/backends.md`, "The manual procedure".
    #[must_use]
    pub fn overlay_enabled(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        unsafe { (client.lib.fns.utils.is_overlay_enabled)(client.utils) }
    }

    /// Whether Steam and its overlay are in Big Picture mode, where the
    /// player most likely has a controller rather than a keyboard
    /// (`ISteamUtils::IsSteamInBigPictureMode`).
    #[must_use]
    pub fn big_picture(&self) -> bool {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        unsafe { (client.lib.fns.utils.is_steam_in_big_picture_mode)(client.utils) }
    }

    /// The language the Steam client's own interface runs in
    /// (`ISteamUtils::GetSteamUILanguage`). Valve's header says a game
    /// probably wants [`Apps::game_language`](crate::Apps::game_language)
    /// instead.
    #[must_use]
    pub fn ui_language(&self) -> String {
        let client = &self.steam.client;
        // SAFETY: see `app_id`; the string is copied before any other Steam
        // call.
        unsafe {
            let language = (client.lib.fns.utils.get_steam_ui_language)(client.utils);
            self.steam.copy_string(language)
        }
    }

    /// The two-letter ISO 3166-1 country code Steam's IP lookup puts this
    /// client in, e.g. `"US"` (`ISteamUtils::GetIPCountry`).
    #[must_use]
    pub fn ip_country(&self) -> String {
        let client = &self.steam.client;
        // SAFETY: as in `ui_language`.
        unsafe {
            let country = (client.lib.fns.utils.get_ip_country)(client.utils);
            self.steam.copy_string(country)
        }
    }

    /// Steam's server time, in seconds since the Unix epoch
    /// (`ISteamUtils::GetServerRealTime`) — a clock the player cannot wind
    /// back by changing the machine's.
    #[must_use]
    pub fn server_unix_time(&self) -> u32 {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        unsafe { (client.lib.fns.utils.get_server_real_time)(client.utils) }
    }

    /// Moves the overlay's notification pop-ups to `corner`
    /// (`ISteamUtils::SetOverlayNotificationPosition`), e.g. off a HUD.
    pub fn set_notification_corner(&self, corner: NotificationCorner) {
        let client = &self.steam.client;
        // SAFETY: see `app_id`; the value is a named `ENotificationPosition`.
        unsafe {
            (client.lib.fns.utils.set_overlay_notification_position)(client.utils, corner.raw())
        }
    }

    /// Insets the notification pop-ups from their corner by this many pixels
    /// (`ISteamUtils::SetOverlayNotificationInset`).
    pub fn set_notification_inset(&self, horizontal: i32, vertical: i32) {
        let client = &self.steam.client;
        // SAFETY: see `app_id`.
        unsafe {
            (client.lib.fns.utils.set_overlay_notification_inset)(
                client.utils,
                horizontal,
                vertical,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::init_on, testing};

    #[test]
    fn hardware_maps_every_named_value_and_keeps_the_rest() {
        assert_eq!(SteamHardware::from_raw(0), SteamHardware::None);
        assert_eq!(SteamHardware::from_raw(1), SteamHardware::SteamDeck);
        assert_eq!(SteamHardware::from_raw(2), SteamHardware::SteamMachine);
        assert_eq!(SteamHardware::from_raw(3), SteamHardware::SteamFrame);
        assert_eq!(SteamHardware::from_raw(4), SteamHardware::Unknown(4));
        assert_eq!(SteamHardware::from_raw(-1), SteamHardware::Unknown(-1));
    }

    #[test]
    fn the_default_config_maps_every_named_value_and_keeps_the_rest() {
        let named = [
            HardwareDefaultConfig::None,
            HardwareDefaultConfig::Low,
            HardwareDefaultConfig::Medium,
            HardwareDefaultConfig::High,
            HardwareDefaultConfig::Max,
            HardwareDefaultConfig::SteamDeck,
            HardwareDefaultConfig::SteamMachine,
            HardwareDefaultConfig::SteamFrame,
        ];
        for (raw, expected) in (0..).zip(named) {
            assert_eq!(HardwareDefaultConfig::from_raw(raw), expected, "{raw}");
        }
        assert_eq!(
            HardwareDefaultConfig::from_raw(8),
            HardwareDefaultConfig::Unknown(8)
        );
        assert_eq!(
            HardwareDefaultConfig::from_raw(-1),
            HardwareDefaultConfig::Unknown(-1)
        );
    }

    #[test]
    fn the_utils_calls_reach_the_client() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        assert_eq!(steam.utils().app_id(), AppId(480));
        testing::script(|s| {
            s.hardware = 1;
            s.default_config = 5;
            s.proton = true;
            s.overlay_enabled = true;
            s.big_picture = true;
            s.server_time = 1_790_000_000;
        });
        let utils = steam.utils();
        assert_eq!(utils.steam_hardware(), SteamHardware::SteamDeck);
        assert_eq!(
            utils.hardware_default_config(),
            HardwareDefaultConfig::SteamDeck
        );
        assert!(utils.under_proton());
        assert!(utils.overlay_enabled());
        assert!(utils.big_picture());
        assert_eq!(utils.server_unix_time(), 1_790_000_000);
        testing::script(|s| s.set_string(b"US"));
        assert_eq!(utils.ip_country(), "US");
        testing::script(|s| s.set_string(b"german"));
        assert_eq!(utils.ui_language(), "german");
    }

    #[test]
    fn notification_placement_passes_valves_values() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        for corner in [
            NotificationCorner::TopLeft,
            NotificationCorner::TopRight,
            NotificationCorner::BottomLeft,
            NotificationCorner::BottomRight,
        ] {
            steam.utils().set_notification_corner(corner);
        }
        steam.utils().set_notification_inset(16, -8);
        let (positions, insets) = testing::script(|s| {
            (
                s.notification_positions.clone(),
                s.notification_insets.clone(),
            )
        });
        assert_eq!(positions, [0, 1, 2, 3]);
        assert_eq!(insets, [(16, -8)]);
    }
}
