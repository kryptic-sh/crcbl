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

/// `ISteamUtils`, borrowed from a [`Steam`]; from [`Steam::utils`].
#[derive(Debug, Clone, Copy)]
pub struct Utils<'a> {
    steam: &'a Steam,
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
        // and Steam stays initialised while `client` lives.
        AppId(unsafe { (client.lib.fns.utils.get_app_id)(client.utils) })
    }

    /// Which Steam hardware this is.
    #[must_use]
    pub fn steam_hardware(&self) -> SteamHardware {
        let client = &self.steam.client;
        // SAFETY: as in `app_id`.
        SteamHardware::from_raw(unsafe {
            (client.lib.fns.utils.is_running_on_steam_hardware)(client.utils)
        })
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
    fn the_utils_calls_reach_the_client() {
        let steam = init_on(testing::fake_lib(), AppId(480)).unwrap();
        assert_eq!(steam.utils().app_id(), AppId(480));
        testing::script(|s| s.hardware = 1);
        assert_eq!(steam.utils().steam_hardware(), SteamHardware::SteamDeck);
    }
}
