//! Why a connection ended.
//!
//! `ESteamNetConnectionEnd` reserves 1000–1999 (`k_ESteamNetConnectionEnd_App_Min`
//! to `App_Max`) for the application. This crate names the reasons a listen
//! host closes a peer with there, so the peer can tell a host that quit from a
//! network that failed; every other code — Steam's own, from a problem on
//! either end, and any app code this crate does not name — is
//! [`EndReason::Lost`], carrying the code.

/// `k_ESteamNetConnectionEnd_App_Min`.
const APP_MIN: i32 = 1000;

/// Why a Steam connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndReason {
    /// The host is leaving the game — a deliberate end, not a failure
    /// (`App_Min + 0`).
    HostLeft,
    /// The host removed this peer (`App_Min + 1`).
    Kicked,
    /// The host had no room (`App_Min + 2`).
    ServerFull,
    /// The other end is shutting its connection down — what a
    /// `SteamTransport` sends when it is dropped (`App_Min + 3`).
    ShuttingDown,
    /// A listener refused a connection from someone not in its lobby
    /// (`App_Min + 4`).
    NotAdmitted,
    /// Anything else: Steam's own codes for a problem detected locally or
    /// remotely, a timeout, or an app code this crate does not name.
    Lost(i32),
}

impl EndReason {
    /// The `ESteamNetConnectionEnd` value sent for this reason.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::HostLeft => APP_MIN,
            Self::Kicked => APP_MIN + 1,
            Self::ServerFull => APP_MIN + 2,
            Self::ShuttingDown => APP_MIN + 3,
            Self::NotAdmitted => APP_MIN + 4,
            Self::Lost(code) => code,
        }
    }

    /// The reason an `m_eEndReason` names.
    #[must_use]
    pub const fn from_code(code: i32) -> Self {
        match code - APP_MIN {
            0 => Self::HostLeft,
            1 => Self::Kicked,
            2 => Self::ServerFull,
            3 => Self::ShuttingDown,
            4 => Self::NotAdmitted,
            _ => Self::Lost(code),
        }
    }

    /// The debug text sent beside the code, for Steam's own logs.
    pub(crate) const fn debug_text(self) -> &'static core::ffi::CStr {
        match self {
            Self::HostLeft => c"host left",
            Self::Kicked => c"kicked",
            Self::ServerFull => c"server full",
            Self::ShuttingDown => c"shutting down",
            Self::NotAdmitted => c"not a member of the lobby",
            Self::Lost(_) => c"",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_reasons_round_trip_through_their_codes() {
        for reason in [
            EndReason::HostLeft,
            EndReason::Kicked,
            EndReason::ServerFull,
            EndReason::ShuttingDown,
            EndReason::NotAdmitted,
        ] {
            assert_eq!(EndReason::from_code(reason.code()), reason);
            assert!(
                (1000..2000).contains(&reason.code()),
                "{reason:?} is in the app range"
            );
        }
        assert_eq!(EndReason::HostLeft.code(), 1000);
    }

    #[test]
    fn steams_own_codes_and_unnamed_app_codes_are_lost() {
        // `k_ESteamNetConnectionEnd_Misc_Timeout` (5001), a remote problem
        // (4000), and an app code past the ones named.
        for code in [0, 4000, 5001, 1005, 1999] {
            assert_eq!(EndReason::from_code(code), EndReason::Lost(code));
        }
    }
}
