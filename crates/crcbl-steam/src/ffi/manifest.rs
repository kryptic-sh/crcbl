//! Every bound function, written once.
//!
//! Each entry names the Rust field, the prototype alias, the exported symbol,
//! **the C declaration it was copied from, verbatim**, and the Rust signature.
//! The `bindings!` macro turns that one list into:
//!
//! - the `prototype` aliases, each documented with its C declaration;
//! - one function-pointer struct per group, and [`Fns`] holding them all;
//! - [`Fns::resolve`], which looks every symbol (and every group's interface
//!   accessor) up through a resolver and fails with `InitError::NoSymbol`
//!   naming the first one missing;
//! - [`INTERFACES`], the accessor rows the handshake is built from;
//! - `BINDINGS` (test builds), the symbol/declaration pairs the drift gate
//!   looks for in the SDK headers.
//!
//! So a function cannot be loaded without being in the table the drift gate
//! reads, and its declaration exists in one place.
//!
//! **Provenance.** Every declaration was written from the SDK 1.65 header
//! mirror (see `docs/plan/42-steam.md`, "Conventions") and has **not** yet
//! been compared with a downloaded SDK: the drift gate has never run, because
//! no machine this was written on had the SDK. Until it has, a declaration
//! here is a claim. Two are worth naming: `ESteamHardwareType` is taken to be
//! an `int`-sized enum, as every Steamworks enum without an explicit base is;
//! and `bool` is C's one-byte `_Bool`, which Rust's `bool` matches across
//! `extern "C"`.

use core::ffi::{c_char, c_void};

use super::{
    HSteamPipe, ISteamUser, ISteamUtils, SteamErrMsg,
    structs::CallbackMsg,
    versions::{self, Interface},
};
use crate::error::InitError;

/// One bound function, as the drift gate looks for it.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoundFn {
    /// The exported symbol.
    pub(crate) symbol: &'static str,
    /// The C declaration, as the header spells it.
    pub(crate) declaration: &'static str,
}

/// Every interface accessor — `ISteamXxx *SteamAPI_SteamXxx_vNNN();` — typed
/// as returning `void *`; each caller casts to its opaque interface type.
pub(crate) type Accessor = unsafe extern "C" fn() -> *mut c_void;

/// Looks a symbol up by name, answering its address or null.
pub(crate) type Resolver<'a> = dyn FnMut(&'static str) -> *mut c_void + 'a;

/// The type of a group's `accessor` field; a macro so `bindings!` can emit
/// the field only for groups that name an interface.
macro_rules! accessor_field {
    ($iface:path) => {
        Accessor
    };
}

/// Expands the one list into prototypes, groups, the resolver and the tables;
/// see the module docs.
macro_rules! bindings {
    ($(
        $(#[$group_doc:meta])*
        $group:ident: $Group:ident $(for $iface:path)? {
            $(
                $field:ident: $Proto:ident = $symbol:literal,
                    $decl:literal,
                    fn($($arg:ty),* $(,)?) $(-> $ret:ty)?;
            )+
        }
    )+) => {
        /// One alias per bound function, each documented with the C
        /// declaration it asserts.
        pub(crate) mod prototype {
            use super::*;
            $($(
                #[doc = concat!("`", $decl, "`")]
                pub(crate) type $Proto = unsafe extern "C" fn($($arg),*) $(-> $ret)?;
            )+)+
        }

        $(
            $(#[$group_doc])*
            #[derive(Debug, Clone, Copy)]
            pub(crate) struct $Group {
                $(
                    /// The interface accessor.
                    pub(crate) accessor: accessor_field!($iface),
                )?
                $(
                    #[doc = concat!("`", $symbol, "`")]
                    pub(crate) $field: prototype::$Proto,
                )+
            }
        )+

        /// Every bound function, by group.
        #[derive(Debug, Clone, Copy)]
        pub(crate) struct Fns {
            $(
                $(#[$group_doc])*
                pub(crate) $group: $Group,
            )+
        }

        impl Fns {
            /// Resolves every symbol and accessor through `resolve`.
            ///
            /// # Errors
            ///
            /// `InitError::NoSymbol` naming the first symbol `resolve`
            /// answered null for — an SDK older than these declarations.
            pub(crate) fn resolve(resolve: &mut Resolver<'_>) -> Result<Self, InitError> {
                Ok(Self {
                    $(
                        $group: $Group {
                            $(accessor: resolve_accessor(resolve, &$iface)?,)?
                            $(
                                $field: {
                                    let raw = resolve($symbol);
                                    if raw.is_null() {
                                        return Err(InitError::NoSymbol($symbol));
                                    }
                                    // SAFETY: the alias is this manifest's
                                    // declaration of exactly this symbol, and a
                                    // function pointer and a data pointer are
                                    // the same size on every supported target.
                                    // The drift gate checks the declaration
                                    // against the SDK's.
                                    unsafe { core::mem::transmute::<*mut c_void, prototype::$Proto>(raw) }
                                },
                            )+
                        },
                    )+
                })
            }
        }

        /// The interface rows the bound groups name, in manifest order — what
        /// the init handshake is built from.
        pub(crate) const INTERFACES: &[Interface] = &[$($($iface,)?)+];

        /// Every bound function's symbol and declaration, for the drift gate.
        #[cfg(test)]
        pub(crate) const BINDINGS: &[BoundFn] = &[
            $($(BoundFn { symbol: $symbol, declaration: $decl },)+)+
        ];
    };
}

/// Resolves one interface accessor.
fn resolve_accessor(resolve: &mut Resolver<'_>, iface: &Interface) -> Result<Accessor, InitError> {
    let raw = resolve(iface.accessor);
    if raw.is_null() {
        return Err(InitError::NoSymbol(iface.accessor));
    }
    // SAFETY: every accessor has the one shape `Accessor` declares (no
    // arguments, an interface pointer back), and function and data pointers
    // are the same size on every supported target.
    Ok(unsafe { core::mem::transmute::<*mut c_void, Accessor>(raw) })
}

bindings! {
    /// Init, shutdown and the pipe (`steam_api.h`, `steam_api_common.h`).
    lifecycle: LifecycleFns {
        init: SteamApiInit = "SteamInternal_SteamAPI_Init",
            "S_API ESteamAPIInitResult S_CALLTYPE SteamInternal_SteamAPI_Init( const char *pszInternalCheckInterfaceVersions, SteamErrMsg *pOutErrMsg );",
            fn(*const c_char, *mut SteamErrMsg) -> i32;
        shutdown: Shutdown = "SteamAPI_Shutdown",
            "S_API void S_CALLTYPE SteamAPI_Shutdown();",
            fn();
        get_pipe: GetHSteamPipe = "SteamAPI_GetHSteamPipe",
            "S_API HSteamPipe S_CALLTYPE SteamAPI_GetHSteamPipe();",
            fn() -> HSteamPipe;
        release_thread_memory: ReleaseCurrentThreadMemory = "SteamAPI_ReleaseCurrentThreadMemory",
            "S_API void S_CALLTYPE SteamAPI_ReleaseCurrentThreadMemory();",
            fn();
    }

    /// Manual callback dispatch (`steam_api.h`). Never mixed with
    /// `SteamAPI_RunCallbacks`, which is deliberately not bound.
    dispatch: DispatchFns {
        init: ManualDispatchInit = "SteamAPI_ManualDispatch_Init",
            "S_API void S_CALLTYPE SteamAPI_ManualDispatch_Init();",
            fn();
        run_frame: ManualDispatchRunFrame = "SteamAPI_ManualDispatch_RunFrame",
            "S_API void S_CALLTYPE SteamAPI_ManualDispatch_RunFrame( HSteamPipe hSteamPipe );",
            fn(HSteamPipe);
        get_next_callback: ManualDispatchGetNextCallback = "SteamAPI_ManualDispatch_GetNextCallback",
            "S_API bool S_CALLTYPE SteamAPI_ManualDispatch_GetNextCallback( HSteamPipe hSteamPipe, CallbackMsg_t *pCallbackMsg );",
            fn(HSteamPipe, *mut CallbackMsg) -> bool;
        free_last_callback: ManualDispatchFreeLastCallback = "SteamAPI_ManualDispatch_FreeLastCallback",
            "S_API void S_CALLTYPE SteamAPI_ManualDispatch_FreeLastCallback( HSteamPipe hSteamPipe );",
            fn(HSteamPipe);
    }

    /// `ISteamUser` (`steam_api_flat.h`).
    user: UserFns for versions::USER {
        get_steam_id: UserGetSteamId = "SteamAPI_ISteamUser_GetSteamID",
            "S_API uint64_steamid SteamAPI_ISteamUser_GetSteamID( ISteamUser* self );",
            fn(*mut ISteamUser) -> u64;
        logged_on: UserBLoggedOn = "SteamAPI_ISteamUser_BLoggedOn",
            "S_API bool SteamAPI_ISteamUser_BLoggedOn( ISteamUser* self );",
            fn(*mut ISteamUser) -> bool;
    }

    /// `ISteamUtils` (`steam_api_flat.h`).
    utils: UtilsFns for versions::UTILS {
        get_app_id: UtilsGetAppId = "SteamAPI_ISteamUtils_GetAppID",
            "S_API AppId_t SteamAPI_ISteamUtils_GetAppID( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> u32;
        is_running_on_steam_hardware: UtilsIsRunningOnSteamHardware = "SteamAPI_ISteamUtils_IsRunningOnSteamHardware",
            "S_API ESteamHardwareType SteamAPI_ISteamUtils_IsRunningOnSteamHardware( ISteamUtils* self );",
            fn(*mut ISteamUtils) -> i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address of a real function, for symbols a test resolves but never
    /// calls.
    extern "C" fn never_called() {}

    fn resolve_all_but(missing: &'static str) -> Result<Fns, InitError> {
        Fns::resolve(&mut |name| {
            if name == missing {
                core::ptr::null_mut()
            } else {
                never_called as *mut c_void
            }
        })
    }

    #[test]
    fn a_missing_function_is_no_symbol_naming_it() {
        let err = resolve_all_but("SteamAPI_ManualDispatch_GetNextCallback").unwrap_err();
        assert_eq!(err, InitError::NoSymbol("SteamAPI_ManualDispatch_GetNextCallback"));
    }

    #[test]
    fn a_missing_accessor_is_no_symbol_naming_it() {
        let err = resolve_all_but(versions::UTILS.accessor).unwrap_err();
        assert_eq!(err, InitError::NoSymbol("SteamAPI_SteamUtils_v011"));
    }

    #[test]
    fn every_symbol_is_resolved_and_every_declaration_names_its_symbol() {
        let mut asked = Vec::new();
        Fns::resolve(&mut |name| {
            asked.push(name);
            never_called as *mut c_void
        })
        .unwrap();
        for bound in BINDINGS {
            assert!(asked.contains(&bound.symbol), "{} never resolved", bound.symbol);
            // A declaration pasted under the wrong symbol would pass the drift
            // gate for the other function and leave this one unchecked.
            assert!(
                bound.declaration.contains(&format!(" {}(", bound.symbol)),
                "{bound:?}"
            );
        }
        for interface in INTERFACES {
            assert!(asked.contains(&interface.accessor), "{interface:?}");
        }
    }
}
