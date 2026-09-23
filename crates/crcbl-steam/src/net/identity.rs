//! `SteamNetworkingIdentity` in its one form this crate uses: a Steam id.

use crate::{SteamId, ffi::structs::SteamNetworkingIdentity};

/// `k_ESteamNetworkingIdentityType_SteamID`.
const STEAM_ID: i32 = 16;

/// The identity naming `user`: type Steam id, 8 bytes in use, the 64-bit id
/// first in the union.
pub(crate) fn of(user: SteamId) -> SteamNetworkingIdentity {
    let mut data = [0; 128];
    data[..8].copy_from_slice(&user.0.to_ne_bytes());
    SteamNetworkingIdentity {
        kind: STEAM_ID,
        size: 8,
        data,
    }
}

/// The Steam id an identity names, if it names one.
pub(crate) fn steam_id(identity: &SteamNetworkingIdentity) -> Option<SteamId> {
    // Copies, never references: the struct is packed.
    let (kind, data) = (identity.kind, identity.data);
    if kind != STEAM_ID {
        return None;
    }
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&data[..8]);
    Some(SteamId(u64::from_ne_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steam_id_round_trips_and_other_kinds_are_none() {
        let identity = of(SteamId(76_561_197_960_287_930));
        assert_eq!({ identity.kind }, 16);
        assert_eq!({ identity.size }, 8);
        assert_eq!(steam_id(&identity), Some(SteamId(76_561_197_960_287_930)));
        let ip = SteamNetworkingIdentity {
            kind: 1,
            ..identity
        };
        assert_eq!(steam_id(&ip), None);
    }
}
