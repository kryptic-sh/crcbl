//! Interface accessors and their interface-version strings.
//!
//! One row per bound interface. Both columns are stored literally and never
//! derived from each other: Valve spells the version strings three different
//! ways (`SteamUser023`, `SteamMatchMaking009` with a capital M the accessor
//! lacks, `STEAMREMOTESTORAGE_INTERFACE_VERSION016`), so a rule that fits one
//! misreads another.
//!
//! The rows a slice binds are the ones `manifest` names; the init handshake
//! is built from exactly those (see [`handshake`]). Read from the SDK 1.65
//! header mirror on 2026-09-22, and checked against it by the drift gate; not
//! yet against an SDK zip from Valve.

/// One interface: how to get it, and which ABI revision that returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Interface {
    /// The flat accessor, e.g. `SteamAPI_SteamUser_v023`. It returns an
    /// interface with exactly that vtable, or null if the running client
    /// cannot provide it.
    pub(crate) accessor: &'static str,
    /// The interface-version string the header `#define`s, e.g.
    /// `SteamUser023`.
    pub(crate) version: &'static str,
    /// The macro that `#define`s [`version`](Self::version), e.g.
    /// `STEAMUSER_INTERFACE_VERSION`; the drift gate reads the header by it.
    pub(crate) define: &'static str,
    /// Whether Valve's own `SteamAPI_InitEx` passes this version to
    /// `SteamInternal_SteamAPI_Init`. Only those go into the handshake:
    /// Valve's list names the client interfaces and not, for instance,
    /// `ISteamTimeline`, and passing a string Valve never passes is untested
    /// ground. An interface outside the list is covered by its accessor's null
    /// check alone.
    pub(crate) in_init_ex: bool,
}

/// `ISteamApps`.
pub(crate) const APPS: Interface = Interface {
    accessor: "SteamAPI_SteamApps_v009",
    version: "STEAMAPPS_INTERFACE_VERSION009",
    define: "STEAMAPPS_INTERFACE_VERSION",
    in_init_ex: true,
};

/// `ISteamFriends`.
pub(crate) const FRIENDS: Interface = Interface {
    accessor: "SteamAPI_SteamFriends_v018",
    version: "SteamFriends018",
    define: "STEAMFRIENDS_INTERFACE_VERSION",
    in_init_ex: true,
};

/// `ISteamUser`.
pub(crate) const USER: Interface = Interface {
    accessor: "SteamAPI_SteamUser_v023",
    version: "SteamUser023",
    define: "STEAMUSER_INTERFACE_VERSION",
    in_init_ex: true,
};

/// `ISteamUtils`.
pub(crate) const UTILS: Interface = Interface {
    accessor: "SteamAPI_SteamUtils_v011",
    version: "SteamUtils011",
    define: "STEAMUTILS_INTERFACE_VERSION",
    in_init_ex: true,
};

/// The `pszInternalCheckInterfaceVersions` argument of
/// `SteamInternal_SteamAPI_Init`: every [`Interface::in_init_ex`] version
/// among `interfaces`, each NUL-terminated, then one more NUL — the list
/// `SteamAPI_InitEx` builds from its `STEAM*_INTERFACE_VERSION` macros.
///
/// A client that cannot honour one of them fails init with
/// `k_ESteamAPIInitResult_VersionMismatch`, rather than handing out an
/// interface whose vtable this crate would misread.
pub(crate) fn handshake(interfaces: &[Interface]) -> Vec<u8> {
    let mut out = Vec::new();
    for interface in interfaces.iter().filter(|i| i.in_init_ex) {
        out.extend_from_slice(interface.version.as_bytes());
        out.push(0);
    }
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row Valve's `InitEx` list does not carry, shaped like the timeline's.
    const OUTSIDE_INIT_EX: Interface = Interface {
        accessor: "SteamAPI_SteamTimeline_v004",
        version: "STEAMTIMELINE_INTERFACE_V004",
        define: "STEAMTIMELINE_INTERFACE_VERSION",
        in_init_ex: false,
    };

    #[test]
    fn the_handshake_is_the_init_ex_versions_nul_joined_and_double_terminated() {
        assert_eq!(
            handshake(&[UTILS, OUTSIDE_INIT_EX, USER]),
            b"SteamUtils011\0SteamUser023\0\0"
        );
    }

    #[test]
    fn an_empty_handshake_is_one_terminator() {
        assert_eq!(handshake(&[OUTSIDE_INIT_EX]), b"\0");
    }

    #[test]
    fn no_version_string_contains_a_nul() {
        // An embedded NUL would end the handshake list early and silently drop
        // every version after it.
        for interface in crate::ffi::manifest::INTERFACES {
            assert!(!interface.version.contains('\0'), "{interface:?}");
        }
    }
}
