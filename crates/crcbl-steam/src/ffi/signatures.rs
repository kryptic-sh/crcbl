//! Each binding's Rust type against its own C declaration.
//!
//! The drift gate compares a binding's `declaration` with the SDK's headers,
//! but the function is called through its Rust type, written separately in
//! the same `manifest` row. Nothing else ties the two: a row whose
//! declaration is right and whose Rust type is not — a `u64` where the header
//! says `uint32`, a missing parameter, `*mut` where Steam reads through a
//! `const` pointer — passes the drift gate, passes every fake-library test
//! (the fake is written from the Rust type), and corrupts the call on a real
//! client. So this translates every declaration's C types into the Rust
//! spelling the manifest must use and compares, on every test run.
//!
//! The translation is deliberately a closed table: a C type it does not know
//! fails the test rather than being guessed at, so a new typedef is mapped
//! once, here, by someone who read its `typedef`.
//!
//! Its tests are ignored under Miri, as the drift gate's own scanner tests
//! are left out: string handling with no `unsafe` in it.

use super::manifest::{BINDINGS, BoundFn};

/// Each C type name a declaration uses, and the Rust type the manifest spells
/// it as. Typedefs map to the alias `ffi` declares for them where there is
/// one, and to the integer their `typedef` names otherwise (read from the SDK
/// 1.65 headers: `typedef uint32 AppId_t;`, `typedef uint32 HAuthTicket;`,
/// `typedef uint32 AccountID_t;`, `typedef uint32 RemotePlaySessionID_t;`,
/// `typedef uint64 uint64_steamid;`). Every `E…` enum is `int`-sized and maps
/// to `i32` by rule, in [`rust_base`].
const TYPES: &[(&str, &str)] = &[
    ("void", "c_void"),
    ("bool", "bool"),
    ("char", "c_char"),
    ("int", "i32"),
    ("int32", "i32"),
    ("int64", "i64"),
    ("uint8", "u8"),
    ("uint16", "u16"),
    ("uint32", "u32"),
    ("uint64", "u64"),
    ("float", "f32"),
    ("double", "f64"),
    ("uint64_steamid", "u64"),
    // `CSteamID` appears only behind a pointer: an out-parameter Steam writes
    // one 64-bit value into.
    ("CSteamID", "u64"),
    ("AppId_t", "u32"),
    ("AccountID_t", "u32"),
    ("HAuthTicket", "u32"),
    ("RemotePlaySessionID_t", "u32"),
    ("HSteamPipe", "HSteamPipe"),
    ("SteamAPICall_t", "SteamApiCall"),
    ("HSteamNetConnection", "HSteamNetConnection"),
    ("HSteamListenSocket", "HSteamListenSocket"),
    ("SteamLeaderboard_t", "SteamLeaderboard"),
    ("SteamLeaderboardEntries_t", "SteamLeaderboardEntries"),
    ("InputHandle_t", "InputHandle"),
    ("InputActionSetHandle_t", "InputActionSetHandle"),
    ("InputDigitalActionHandle_t", "InputDigitalActionHandle"),
    ("InputAnalogActionHandle_t", "InputAnalogActionHandle"),
    ("ScreenshotHandle", "ScreenshotHandle"),
    ("TimelineEventHandle_t", "TimelineEventHandle"),
    ("PublishedFileId_t", "PublishedFileId"),
    ("UGCQueryHandle_t", "UgcQueryHandle"),
    ("UGCUpdateHandle_t", "UgcUpdateHandle"),
    ("SteamInventoryResult_t", "SteamInventoryResult"),
    ("SteamItemInstanceID_t", "SteamItemInstanceId"),
    ("SteamItemDef_t", "SteamItemDef"),
    ("SteamErrMsg", "SteamErrMsg"),
    ("CallbackMsg_t", "CallbackMsg"),
    ("InputAnalogActionData_t", "InputAnalogActionData"),
    ("InputDigitalActionData_t", "InputDigitalActionData"),
    ("LeaderboardEntry_t", "LeaderboardEntry"),
    ("SteamItemDetails_t", "SteamItemDetails"),
    ("SteamNetConnectionInfo_t", "SteamNetConnectionInfo"),
    ("SteamNetworkingIdentity", "SteamNetworkingIdentity"),
    ("SteamNetworkingMessage_t", "SteamNetworkingMessage"),
    ("SteamParamStringArray_t", "SteamParamStringArray"),
    ("SteamRelayNetworkStatus_t", "SteamRelayNetworkStatus"),
    ("SteamUGCDetails_t", "SteamUgcDetails"),
    // Never built here: every call passes null options, so the pointee is
    // opaque.
    ("SteamNetworkingConfigValue_t", "c_void"),
    ("ISteamApps", "ISteamApps"),
    ("ISteamFriends", "ISteamFriends"),
    ("ISteamInput", "ISteamInput"),
    ("ISteamInventory", "ISteamInventory"),
    ("ISteamMatchmaking", "ISteamMatchmaking"),
    ("ISteamNetworkingSockets", "ISteamNetworkingSockets"),
    ("ISteamNetworkingUtils", "ISteamNetworkingUtils"),
    ("ISteamRemotePlay", "ISteamRemotePlay"),
    ("ISteamRemoteStorage", "ISteamRemoteStorage"),
    ("ISteamScreenshots", "ISteamScreenshots"),
    ("ISteamTimeline", "ISteamTimeline"),
    ("ISteamUGC", "ISteamUgc"),
    ("ISteamUser", "ISteamUser"),
    ("ISteamUserStats", "ISteamUserStats"),
    ("ISteamUtils", "ISteamUtils"),
];

/// The Rust spelling of a C type name with no pointer or `const` on it.
fn rust_base(name: &str) -> Result<&'static str, String> {
    if let Some(&(_, rust)) = TYPES.iter().find(|&&(c, _)| c == name) {
        return Ok(rust);
    }
    let enum_like = name.strip_prefix('E').is_some_and(|rest| {
        rest.starts_with(|c: char| c.is_ascii_uppercase())
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    if enum_like {
        Ok("i32")
    } else {
        Err(format!("no Rust spelling for the C type `{name}`"))
    }
}

/// One C type — a parameter with its name removed, or a return type — as
/// the Rust type the manifest must declare. A C++ reference is a pointer at
/// the ABI; each level of pointer is `*const` when the `const` before it
/// qualifies the pointee.
fn rust_type(c: &str) -> Result<String, String> {
    let spaced = c.replace('*', " * ").replace('&', " & ");
    let mut base = None;
    // `const` seen since the last pointer: it qualifies what the next
    // pointer points at.
    let mut pending_const = false;
    let mut pointers: Vec<bool> = Vec::new();
    for token in spaced.split_whitespace() {
        match token {
            "const" => pending_const = true,
            "*" | "&" => {
                pointers.push(pending_const);
                pending_const = false;
            }
            "unsigned" | "signed" | "struct" | "class" => {
                return Err(format!("unhandled C type `{c}`"));
            }
            name if base.is_none() => base = Some(name),
            _ => return Err(format!("more than one type name in `{c}`")),
        }
    }
    let base = base.ok_or_else(|| format!("no type name in `{c}`"))?;
    let mut rust = rust_base(base)?.to_owned();
    for pointee_const in pointers {
        rust = format!("{} {rust}", if pointee_const { "*const" } else { "*mut" });
    }
    Ok(rust)
}

/// A parameter declaration without its name: `const char * pchKey` is
/// `const char *`. A lone type (`void`) is kept whole.
fn parameter_type(parameter: &str) -> &str {
    let trimmed = parameter.trim_end();
    let name_start = trimmed
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .map_or(0, |at| at + 1);
    let head = trimmed[..name_start].trim_end();
    if head.is_empty() || head == "const" {
        trimmed
    } else {
        head
    }
}

/// The Rust function-pointer type a declaration calls for, in `stringify!`'s
/// shape: `fn(A, B) -> R`, with no `-> ()` for `void`.
fn expected(bound: &BoundFn) -> Result<String, String> {
    let declaration = bound
        .declaration
        .trim()
        .strip_suffix(';')
        .ok_or("the declaration does not end in `;`")?;
    let open = declaration
        .find(&format!(" {}(", bound.symbol))
        .ok_or("the declaration does not name its symbol")?;
    let head = declaration[..open]
        .trim()
        .strip_prefix("S_API")
        .ok_or("the declaration does not start with `S_API`")?;
    let returns = head.replace("S_CALLTYPE", "");
    let parameters = declaration[open + bound.symbol.len() + 2..]
        .trim_end()
        .strip_suffix(')')
        .ok_or("the parameter list is not closed")?;
    let mut arguments = Vec::new();
    for parameter in parameters.split(',').map(str::trim) {
        if parameter.is_empty() || parameter == "void" {
            continue;
        }
        arguments.push(rust_type(parameter_type(parameter))?);
    }
    let mut out = format!("fn({})", arguments.join(", "));
    if returns.trim() != "void" {
        out.push_str(" -> ");
        out.push_str(&rust_type(&returns)?);
    }
    Ok(out)
}

/// Whitespace removed: `stringify!` spaces tokens its own way.
fn squash(text: &str) -> String {
    text.split_whitespace().collect()
}

/// Every binding whose Rust type is not what its declaration calls for.
fn check(bindings: &[BoundFn]) -> Vec<String> {
    let mut failures = Vec::new();
    for bound in bindings {
        match expected(bound) {
            Ok(want) if squash(&want) == squash(bound.rust) => {}
            Ok(want) => failures.push(format!(
                "{}: declared `{}`, called as `{}`; the declaration calls for `{want}`",
                bound.symbol, bound.declaration, bound.rust
            )),
            Err(why) => failures.push(format!("{}: {why}", bound.symbol)),
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use core::any::TypeId;

    use super::*;
    use crate::ffi::{
        HSteamListenSocket, HSteamNetConnection, HSteamPipe, HSteamUser, InputActionSetHandle,
        InputAnalogActionHandle, InputDigitalActionHandle, InputHandle, PublishedFileId,
        ScreenshotHandle, SteamApiCall, SteamInventoryResult, SteamItemDef, SteamItemInstanceId,
        SteamLeaderboard, SteamLeaderboardEntries, TimelineEventHandle, UgcQueryHandle,
        UgcUpdateHandle,
    };

    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn every_bindings_rust_type_is_what_its_declaration_calls_for() {
        let failures = check(BINDINGS);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert!(BINDINGS.len() > 100, "{} bindings checked", BINDINGS.len());
    }

    /// The aliases [`TYPES`] maps typedefs to are the integers the SDK's
    /// `typedef`s name, so mapping to the alias is mapping to the integer.
    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn every_alias_is_the_integer_its_typedef_names() {
        fn same<A: 'static, B: 'static>() -> bool {
            TypeId::of::<A>() == TypeId::of::<B>()
        }
        assert!(same::<HSteamPipe, i32>());
        assert!(same::<HSteamUser, i32>());
        assert!(same::<SteamApiCall, u64>());
        assert!(same::<HSteamNetConnection, u32>());
        assert!(same::<HSteamListenSocket, u32>());
        assert!(same::<SteamLeaderboard, u64>());
        assert!(same::<SteamLeaderboardEntries, u64>());
        assert!(same::<InputHandle, u64>());
        assert!(same::<InputActionSetHandle, u64>());
        assert!(same::<InputDigitalActionHandle, u64>());
        assert!(same::<InputAnalogActionHandle, u64>());
        assert!(same::<ScreenshotHandle, u32>());
        assert!(same::<TimelineEventHandle, u64>());
        assert!(same::<PublishedFileId, u64>());
        assert!(same::<UgcQueryHandle, u64>());
        assert!(same::<UgcUpdateHandle, u64>());
        assert!(same::<SteamInventoryResult, i32>());
        assert!(same::<SteamItemInstanceId, u64>());
        assert!(same::<SteamItemDef, i32>());
    }

    fn doctored(rust: &'static str) -> Vec<String> {
        let mut bound = *BINDINGS
            .iter()
            .find(|bound| bound.symbol == "SteamAPI_ISteamUtils_GetAppID")
            .unwrap();
        assert_eq!(check(&[bound]), Vec::<String>::new());
        bound.rust = rust;
        check(&[bound])
    }

    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn a_wrong_return_type_fails() {
        assert_eq!(doctored("fn(*mut ISteamUtils) -> u64").len(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn a_missing_or_extra_parameter_fails() {
        assert_eq!(doctored("fn() -> u32").len(), 1);
        assert_eq!(doctored("fn(*mut ISteamUtils, i32) -> u32").len(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn pointer_constness_is_compared() {
        let bound = BoundFn {
            symbol: "SteamAPI_ISteamFriends_SetRichPresence",
            declaration: "S_API bool SteamAPI_ISteamFriends_SetRichPresence( ISteamFriends* self, const char * pchKey, const char * pchValue );",
            rust: "fn(*mut ISteamFriends, *const c_char, *const c_char) -> bool",
        };
        assert_eq!(check(&[bound]), Vec::<String>::new());
        let mutable = BoundFn {
            rust: "fn(*mut ISteamFriends, *mut c_char, *const c_char) -> bool",
            ..bound
        };
        assert_eq!(check(&[mutable]).len(), 1);
    }

    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn references_and_pointers_to_pointers_translate() {
        assert_eq!(
            rust_type("const SteamNetworkingIdentity &").unwrap(),
            "*const SteamNetworkingIdentity"
        );
        assert_eq!(rust_type("const char **").unwrap(), "*mut *const c_char");
        assert_eq!(rust_type("void * const").unwrap(), "*mut c_void");
        assert_eq!(parameter_type("const char * pchKey"), "const char *");
        assert_eq!(parameter_type("EResult"), "EResult");
    }

    #[test]
    #[cfg_attr(miri, ignore = "string handling with no unsafe")]
    fn an_unknown_c_type_fails_rather_than_being_guessed() {
        let bound = BoundFn {
            symbol: "SteamAPI_Made_Up",
            declaration: "S_API FooBar_t SteamAPI_Made_Up( ISteamUtils* self );",
            rust: "fn(*mut ISteamUtils) -> u32",
        };
        let failures = check(&[bound]);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("FooBar_t"), "{failures:?}");
    }
}
