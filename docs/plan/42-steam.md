# Topic 42 — Steamworks (`crcbl-steam`)

Steam support as a workspace crate: an FFI wrapper over the Steamworks SDK's
flat C API that is idiomatic Rust at the surface — an event queue drained once
per frame, typed tokens for asynchronous calls, errors that name the failure —
and hand-written FFI underneath, in the shape `crcbl-shell` already uses for
libxcb and libwayland-client. This document fixes how the SDK is obtained (it
cannot be vendored), how the crate stays green on a CI that has no Steam client
and a workspace that compiles every crate on four targets, how Steam's callback
model maps onto the engine's frame loop, which engine seams each Steam feature
plugs into, and — since 2026-09-22 — **the whole Steamworks surface a game uses,
on Linux, Windows and macOS, cut into ordered slices** that an implementer can
land one at a time, each with tests that can fail and a stated manual check
against a real Steam client.

Like topics 11–41 its number is identity, not sequence. The topic row already
exists in `00-overview.md`; claiming a phase in `ROADMAP.md` belongs to slice 1.

**Status (2026-09-23): slices 1 and 1b built on branch `steam-sdk`, the rest
planned** — see "Status by slice" under "Slice order". The four decisions the
earlier draft asked for were ratified 2026-09-06 (see "Decisions" below), and
"the full Steam API" is now in scope, which reverses two earlier "not now" calls
— Steam Input and `SteamTransport` — and pulls the first consumer's requirements
(the game EW, below) forward in the slice order. The plan was reviewed the same
day against the SDK 1.65 headers and this tree; "Review (step 2)" at the end
lists what that changed, including EW's answers to the questions the first draft
left open.

Two findings shape everything below, so they come first:

- **The SDK cannot enter the repo, and CI can never run it.** Valve's access
  agreement licenses redistribution of `redistributable_bin` in object form
  alongside a shipped game and nothing else; the headers are licensed for local
  reproduction "solely to develop the Licensee Software". The SDK zip itself is
  behind a partner-site login (verified: the download URL answers a 302 to the
  login page). So there is no vendored header, no `build.rs` that links a
  `.lib`, and no CI job that talks to Steam — the whole design has to be
  arranged so that what CI _can_ check is real and what it cannot check is
  stated rather than faked.
- **Steam is a backend the project does not have to run.** The 2026-08-09
  roadmap correction cut hosted token auth and `crcbl-mint` because "no hosted
  infrastructure exists anywhere in the project". Steam's relay network,
  lobbies, cloud and identity are all operated by Valve. Everything planned here
  keeps that property; the one Steamworks feature that would break it
  (microtransactions, which need a server holding a publisher key) is declined
  for exactly that reason.

## Conventions in this document

- **Paths inside the proposed crate are written crate-relative**
  (`crates/crcbl-steam/src/pump.rs`), because the crate does not exist yet and
  `tools/check-doc-citations.sh` checks only paths rooted at a top-level
  directory. The slice that creates a file switches its citations here to the
  rooted `crates/crcbl-steam/…` form in the same commit, so the gate starts
  checking them the moment they can resolve.
- **Paths inside the Steamworks SDK** (`public/steam/steam_api_flat.h`,
  `redistributable_bin/…`) are external and never rooted at a repository
  directory, which is the opt-out: the gate does not ask them to exist.
- **Flat-API facts** (function names, accessor versions, struct packing,
  callback ids) were re-read on 2026-09-22 from the header mirror Steamworks.NET
  maintains for its code generator
  ([rlabrecque/Steamworks.NET `CodeGen/steam`](https://github.com/rlabrecque/Steamworks.NET/tree/master/CodeGen/steam)),
  because the SDK zip is login-gated. That mirror was last updated 2026-08-07 by
  the commit "Update to Steamworks 1.65[a]", so the facts here are **SDK
  1.65's**. Every one must be re-read from the SDK the implementer downloads;
  the drift gate (slice 1) is what makes that mechanical.
- **"Default (user to confirm)"** marks a decision this plan took on the user's
  behalf while they were away. Each is collected again under "Defaulted
  decisions" at the end, with the alternative and what changing it would cost.

## The first consumer: EW's requirements

EW (a crcbl game, co-op, player-hosted) sent its Steamworks requirements on
2026-09-22, and answered the first draft's open questions the same day (see
"Build order" under "Slice order"). They decide the order: every **hard**
requirement is satisfied by the time slice 7b lands, and nothing EW does not use
is built ahead of one it does. Slice numbers are identity, not sequence — the
build order is stated separately.

| #   | EW requirement                                                                                                                                                                                                            | Weight              | Satisfied by                                                                                                                                                                                             |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Listen-server co-op, squads of up to four friends, host authoritative; Steam networking (sockets, relay, P2P) as transport; friends/invites as the invitation path; reconnect after disconnect; clean host-leave handling | hard                | slice 3a (lobbies, invites, join paths) + slice 4 (`SteamTransport`, listener, end reasons, reconnect) + slice 2 (the transport-generic multi-session host: N peers, per-peer resume, host-left vs lost) |
| 2   | No anti-cheat, no encrypted app tickets                                                                                                                                                                                   | hard (a "not")      | auth and encrypted tickets drop to slice 12; peer identity comes from the connection's Steam-certified identity instead (slice 4)                                                                        |
| 3   | Voice as raw decoded PCM into the game's own mixer; push-to-talk under game control; never Steam's playback                                                                                                               | hard                | slice 5                                                                                                                                                                                                  |
| 4   | Steam Input (Deck) arrives as the same `crcbl-input` device events as evdev/XInput, into backend-neutral snapshots, not a separate path                                                                                   | hard                | slice 7a (the gamepad seam in `crcbl-input`) + slice 7b (Steam Input onto it)                                                                                                                            |
| 5   | Cloud: whole-file sync of one atomically written profile file; conflicts surfaced to the game; never merged or partially applied                                                                                          | hard                | slice 6 (`SteamCloudStorage` + the synced-file protocol in `crcbl-store`). Auto-Cloud cannot meet it — see "Cloud". EW confirmed the `ISteamRemoteStorage` backend 2026-09-22                            |
| 6   | Local Steam ID as a stable profile and participant identity                                                                                                                                                               | hard                | slice 1 (`steam.user().steam_id()`), and slice 4 for the remote side                                                                                                                                     |
| —   | Rich presence, achievements and stats, overlay and friends list for invites                                                                                                                                               | wanted              | slice 3a (the `connect` presence key, invite dialog), slice 3b (persona, friends list, avatars, other overlay dialogs), slice 9 (achievements, stats — optional for EW)                                  |
| —   | Workshop, inventory, leaderboards, timeline, DLC                                                                                                                                                                          | unused by EW, scope | slices 9–15                                                                                                                                                                                              |

## What the flat API actually is

- **`steam_api_flat.h` is a plain-C mirror of every interface method**,
  auto-generated by Valve, "for binding to other languages"
  ([API overview](https://partner.steamgames.com/doc/sdk/api)). Methods take the
  interface pointer explicitly and use C types throughout — `CSteamID` crosses
  as `uint64_steamid`:

  ```c
  S_API uint64_steamid SteamAPI_ISteamUser_GetSteamID( ISteamUser* self );
  S_API bool SteamAPI_ISteamUserStats_SetAchievement( ISteamUserStats* self, const char * pchName );
  S_API bool SteamAPI_ISteamRemoteStorage_FileWrite( ISteamRemoteStorage* self, const char * pchFile, const void * pvData, int32 cubData );
  S_API SteamAPICall_t SteamAPI_ISteamUGC_SubmitItemUpdate( ISteamUGC* self, UGCUpdateHandle_t handle, const char * pchChangeNote );
  ```

  "Plain C" has two exceptions the binding must know about. Some flat functions
  take a C++ reference — `SteamAPI_ISteamNetworkingSockets_ConnectP2P` takes
  `const SteamNetworkingIdentity & identityRemote` — which is a pointer at the
  ABI and is declared as `*const SteamNetworkingIdentity`. And some **return a
  struct by value**: `SteamAPI_ISteamInput_GetAnalogActionData` returns
  `InputAnalogActionData_t`, declared under `#pragma pack( push, 1 )`. Both are
  called out again under "The cases that are easy to get wrong".

- **Interface pointers come from version-suffixed accessors**, one per interface
  per ABI revision. As read from the 1.65 mirror on 2026-09-22 (every accessor
  name checked against `steam_api_flat.h`, every version string against the
  interface header's `#define` and `steam_api.json`'s `version_string`):

  | Interface                  | Accessor                                         | Interface-version string                   | Game-server twin                                           |
  | -------------------------- | ------------------------------------------------ | ------------------------------------------ | ---------------------------------------------------------- |
  | `ISteamUser`               | `SteamAPI_SteamUser_v023`                        | `SteamUser023`                             | —                                                          |
  | `ISteamFriends`            | `SteamAPI_SteamFriends_v018`                     | `SteamFriends018`                          | —                                                          |
  | `ISteamUtils`              | `SteamAPI_SteamUtils_v011`                       | `SteamUtils011`                            | `SteamAPI_SteamGameServerUtils_v011`                       |
  | `ISteamMatchmaking`        | `SteamAPI_SteamMatchmaking_v009`                 | `SteamMatchMaking009`                      | —                                                          |
  | `ISteamMatchmakingServers` | `SteamAPI_SteamMatchmakingServers_v003`          | `SteamMatchMakingServers003`               | —                                                          |
  | `ISteamParties`            | `SteamAPI_SteamParties_v002`                     | `SteamParties002`                          | —                                                          |
  | `ISteamRemoteStorage`      | `SteamAPI_SteamRemoteStorage_v016`               | `STEAMREMOTESTORAGE_INTERFACE_VERSION016`  | —                                                          |
  | `ISteamUserStats`          | `SteamAPI_SteamUserStats_v013`                   | `STEAMUSERSTATS_INTERFACE_VERSION013`      | `SteamAPI_SteamGameServerStats_v001`                       |
  | `ISteamApps`               | `SteamAPI_SteamApps_v009`                        | `STEAMAPPS_INTERFACE_VERSION009`           | —                                                          |
  | `ISteamScreenshots`        | `SteamAPI_SteamScreenshots_v003`                 | `STEAMSCREENSHOTS_INTERFACE_VERSION003`    | —                                                          |
  | `ISteamInput`              | `SteamAPI_SteamInput_v007`                       | `SteamInput007`                            | —                                                          |
  | `ISteamUGC`                | `SteamAPI_SteamUGC_v021`                         | `STEAMUGC_INTERFACE_VERSION021`            | `SteamAPI_SteamGameServerUGC_v021`                         |
  | `ISteamInventory`          | `SteamAPI_SteamInventory_v003`                   | `STEAMINVENTORY_INTERFACE_V003`            | `SteamAPI_SteamGameServerInventory_v003`                   |
  | `ISteamTimeline`           | `SteamAPI_SteamTimeline_v004`                    | `STEAMTIMELINE_INTERFACE_V004` (see below) | —                                                          |
  | `ISteamRemotePlay`         | `SteamAPI_SteamRemotePlay_v004`                  | `STEAMREMOTEPLAY_INTERFACE_VERSION004`     | —                                                          |
  | `ISteamNetworkingSockets`  | `SteamAPI_SteamNetworkingSockets_SteamAPI_v013`  | `SteamNetworkingSockets013`                | `SteamAPI_SteamGameServerNetworkingSockets_SteamAPI_v013`  |
  | `ISteamNetworkingMessages` | `SteamAPI_SteamNetworkingMessages_SteamAPI_v002` | `SteamNetworkingMessages002`               | `SteamAPI_SteamGameServerNetworkingMessages_SteamAPI_v002` |
  | `ISteamNetworkingUtils`    | `SteamAPI_SteamNetworkingUtils_SteamAPI_v004`    | `SteamNetworkingUtils004`                  | (shared)                                                   |
  | `ISteamGameServer`         | `SteamAPI_SteamGameServer_v015`                  | `SteamGameServer015`                       | —                                                          |

  **The version strings follow no single pattern** — `SteamMatchMaking009` has a
  capital M the accessor lacks, `STEAMINVENTORY_INTERFACE_V003` and
  `STEAMREMOTESTORAGE_INTERFACE_VERSION016` are spelled three different ways —
  so `versions.rs` stores both columns literally and never derives one from the
  other.

  The suffix is the contract: an accessor that exists returns an interface with
  exactly that vtable layout, and one the running client cannot honour returns
  null rather than the wrong table. Every accessor result is null-checked at
  init — a null is `InitError::NoInterface(name)`, never a stored null pointer.

- **Initialisation has a flat entry point that skips version checking, and an
  internal one that does the checking.** `steam_api.h`:

  ```c
  // Same usage as SteamAPI_InitEx(), however does not verify ISteam* interfaces are
  // supported by the user's client and is exported from the dll
  S_API ESteamAPIInitResult S_CALLTYPE SteamAPI_InitFlat( SteamErrMsg *pOutErrMsg );
  S_API ESteamAPIInitResult S_CALLTYPE SteamInternal_SteamAPI_Init( const char *pszInternalCheckInterfaceVersions, SteamErrMsg *pOutErrMsg );
  ```

  The C++-only `SteamAPI_InitEx` is an inline that calls
  `SteamInternal_SteamAPI_Init` with a `"\0"`-joined list of every
  `STEAM*_INTERFACE_VERSION` string baked in at compile time. **`crcbl-steam`
  calls `SteamInternal_SteamAPI_Init` with exactly the interface-version strings
  of the accessors it binds** (`"SteamUtils011\0SteamUser023\0…\0\0"`, derived
  from one table in `crates/crcbl-steam/src/ffi/versions.rs`, never typed
  twice), so a client that cannot honour them fails init with
  `k_ESteamAPIInitResult_VersionMismatch` and an English `SteamErrMsg`
  (`typedef char SteamErrMsg[1024]`). `ESteamAPIInitResult` is
  `OK = 0, FailedGeneric = 1, NoSteamClient = 2, VersionMismatch = 3`.
  `SteamAPI_InitFlat` is not bound. **One restriction:** Valve's own `InitEx`
  list (`steam_api.h`, 1.65) names the client interfaces only — it has no
  `STEAMTIMELINE_INTERFACE_VERSION` and none of the game-server ones — so the
  handshake string carries only versions that appear in that list, and the rest
  (timeline) are covered by the accessor null check alone. Passing a string
  Valve's own list never passes is untested ground this plan does not need to
  stand on.

- **Manual dispatch replaces the C++ callback machinery entirely.** The header
  forbids mixing the two paths ("If you use the manual callback dispatch, you
  must NOT use: SteamAPI_RunCallbacks … STEAM_CALLBACK, CCallResult, CCallback,
  or CCallbackManual"):

  ```c
  S_API void S_CALLTYPE SteamAPI_ManualDispatch_Init();
  S_API void S_CALLTYPE SteamAPI_ManualDispatch_RunFrame( HSteamPipe hSteamPipe );
  S_API bool S_CALLTYPE SteamAPI_ManualDispatch_GetNextCallback( HSteamPipe hSteamPipe, CallbackMsg_t *pCallbackMsg );
  S_API void S_CALLTYPE SteamAPI_ManualDispatch_FreeLastCallback( HSteamPipe hSteamPipe );
  S_API bool S_CALLTYPE SteamAPI_ManualDispatch_GetAPICallResult( HSteamPipe hSteamPipe, SteamAPICall_t hSteamAPICall, void *pCallback, int cubCallback, int iCallbackExpected, bool *pbFailed );
  ```

  Each drained message is a `CallbackMsg_t` —
  `{ HSteamUser m_hSteamUser; int m_iCallback; uint8 *m_pubParam; int m_cubParam; }`
  — an integer id plus a borrowed byte buffer, valid until `FreeLastCallback`.
  It is declared in `steam_api_internal.h` (not `steam_api.h`) under the
  platform callback packing, so it is **20 bytes on Linux and macOS (`pack(4)`:
  the trailing `int` gets no padding) and 24 on Windows (`pack(8)`)** — the
  first struct slice 1 declares is already one whose size differs by OS. It is
  not in `steam_api.json`. Asynchronous results arrive as
  `SteamAPICallCompleted_t` (id `k_iSteamUtilsCallbacks + 3`,
  `k_iSteamUtilsCallbacks = 700`) carrying
  `{ SteamAPICall_t m_hAsyncCall; int m_iCallback; uint32 m_cubParam; }`, and
  the payload is fetched with `GetAPICallResult`. `SteamAPICall_t` is `uint64`
  with `0` reserved as `k_uAPICallInvalid`; `HSteamPipe` and `HSteamUser` are
  `int32`. The pipe comes from `SteamAPI_GetHSteamPipe()` (the game-server pipe
  from `SteamGameServer_GetHSteamPipe()`), both declared in
  `steam_api_internal.h`. **This is the ABI `crcbl-steam` binds — bytes and
  integers on a pipe we poll, no C++ vtables registered with anyone, and no
  foreign code calling back into Rust.**

- **Calling convention.** `S_CALLTYPE` is `__cdecl` and the flat functions carry
  none; on every 64-bit target in scope there is one C convention, so every
  pointer is `unsafe extern "C" fn`.
- **Manual dispatch replaces `SteamAPI_RunCallbacks`, and with it one of its
  side effects.** `RunCallbacks` calls `SteamAPI_ReleaseCurrentThreadMemory`
  automatically
  ([steam_api.h reference](https://partner.steamgames.com/doc/api/steam_api):
  "This function is called automatically by SteamAPI_RunCallbacks, so a program
  that only ever accesses the Steamworks API from a single thread never needs to
  explicitly call this function"). Nothing says `ManualDispatch_RunFrame` does,
  so `pump` calls `ReleaseCurrentThreadMemory` itself at the end of every drain.
- **The SDK ships `steam_api.json`**, a machine-readable description of every
  interface method, callback struct, constant and interface-version string. It
  is **not** complete enough to be the drift gate's only input: it has no entry
  for the lifecycle functions (`SteamInternal_SteamAPI_Init`,
  `SteamAPI_ManualDispatch_*`, `SteamAPI_RestartAppIfNecessary`,
  `SteamAPI_Shutdown`, `SteamAPI_GetHSteamPipe`), none for `CallbackMsg_t`, and
  no struct sizes. The gate therefore reads the headers (see "The drift gate").
  The Steamworks.NET mirror carries a copy; this repository does not.
- **Version facts.** Current SDK is **1.65** — the Steamworks.NET mirror moved
  to it 2026-07-26 ("Update to Steamworks 1.65") and to a 1.65a refresh
  2026-08-07; 1.64 landed there 2026-03-13. 1.63 (2026-01-29:
  [announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/627817201164877826))
  added linuxarm64/androidarm64 libs and removed `ISteamMusicRemote`; 1.61
  removed `RequestCurrentStats`
  ([1.61 announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/4480612432780198328));
  1.62 removed `ISteamFriends::SetPersonaName` and `GetUserRestrictions`
  ([1.62 notes](https://steamdb.info/patchnotes/17946746/)). By 1.65
  `ISteamUtils::IsSteamRunningOnSteamDeck` is gone from `SteamUtils011`,
  replaced by `IsRunningOnSteamHardware()` returning `ESteamHardwareType`
  (`None = 0, SteamDeck = 1, SteamMachine = 2, SteamFrame = 3`), beside
  `GetSteamHardwareDefaultConfig()` and `IsRunningUnderProton()`. Read the
  number off the zip the implementer downloads.

## The licence, and where the SDK comes from

What the
[SDK Access Agreement](https://partner.steamgames.com/documentation/sdk_access_agreement)
permits, in its own words: Valve grants a licence to "reproduce and distribute
the part of the SDK provided inside the folder named redistributable_bin (the
'SDK Redistributables') along with the Licensee Software in object code form",
and to "use and locally reproduce the SDK in source code form, solely to develop
the Licensee Software". Nothing grants publication of the headers, and this repo
is public under MIT.

- **Nothing from the SDK is committed.** No header, no `steam_api.json`, no
  redistributable. The repo carries only our own declarations of the C ABI —
  function prototypes and struct layouts written by hand, each carrying the SDK
  version it was read from. Precedent: Steamworks.NET has published its own
  flat-API declarations under MIT for a decade. (Ratified 2026-09-06.)
- **A developer supplies the SDK themselves**: download the zip from the partner
  site (any Steam account that has accepted the agreement), unzip anywhere, and
  set `CRCBL_STEAM_SDK=/path/to/sdk`. Nothing reads it at _build_ time. It is
  read by (1) the drift gate, from the headers in `public/steam/`, and (2) the
  runtime library search, from `redistributable_bin/<platform>/`, as the
  development fallback after "next to the executable".
- **CI never has the SDK.** Default (user to confirm): no CI job fetches it —
  not from a secret, not from a private mirror, and not from Steamworks.NET's
  public copy of the headers, whose own licence posture this project should not
  lean on. The drift gate is therefore a local gate, run by the developer who
  touches the declarations (see "What CI proves"). The gate reads whatever
  directory `CRCBL_STEAM_SDK` names, so flipping this later is a CI-step change,
  not a code change.
- **`.gitignore` gains `steam_appid.txt` and the SDK directory names**
  (`steamworks_sdk*/`, `sdk/redistributable_bin/`), so neither a dev app-id file
  nor an unzipped SDK dropped into the checkout can be committed by accident.
- **Shipping** (slice 15): the game's depot carries the redistributable next to
  the executable — the one distribution the agreement licenses — and
  `steam_appid.txt` must not ship ("Do not ship this with your builds",
  [API overview](https://partner.steamgames.com/doc/sdk/api)).

## Platforms: loading the library

**Decision: runtime dynamic loading, never link-time.** Recommended and adopted
by this plan, weighed against the alternative:

| Route                                                    | New crates.io dependency             | Build complexity                                                                                                                           | Machine without Steam                                                  | CI                                         |
| -------------------------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------- | ------------------------------------------ |
| **`dlopen`/`LoadLibraryExW` at `Steam::init` (adopted)** | none                                 | one loader module per OS family; `dlopen`/`dlsym` declared against libc as `x11/ffi.rs` does, `LoadLibraryExW`/`GetProcAddress` on Windows | `Err(InitError::NoLibrary)` — the game runs without Steam              | builds and tests everywhere with no SDK    |
| `#[link(name = "steam_api")]` / `cargo:rustc-link-lib`   | none                                 | needs `steam_api64.lib` (Windows import library) or the `.so`/`.dylib` at link time — SDK files CI cannot have                             | dies in the dynamic loader before `main`, before any fall-through runs | cannot build: the link inputs are SDK-only |
| `steamworks-rs` (links, vendors the SDK)                 | yes (`steamworks`, `steamworks-sys`) | none for us                                                                                                                                | same death as `#[link]`                                                | builds (SDK vendored inside the crate)     |

Runtime loading is the only route that satisfies "no new dependency" _and_ lets
CI build the crate at all; it is also what `crcbl-vk` does for the Vulkan
loader, for the reasons its crate docs give (`crates/crcbl-vk/src/lib.rs`: a
fallible entry, one binary across a matrix that mostly lacks the library, one
`dlopen` of cost). The Windows shell links its system DLLs instead, and
`crates/crcbl-shell/src/win32/ffi.rs` says why that argument does not transfer:
`user32.dll` cannot be absent, and `steam_api64.dll` usually is.

**The library is always opened by absolute path**, never by bare name, and the
search order is fixed and reported in full on failure:

1. The directory of `std::env::current_exe()` — where a shipped build places the
   redistributable (Valve's documented layout). On macOS, additionally
   `../Frameworks/` relative to the executable, for an `.app` bundle.
2. `$CRCBL_STEAM_SDK/redistributable_bin/<platform>/` — development only.

| Target                                        | File                        | SDK directory               | Status in this plan                                                                                               |
| --------------------------------------------- | --------------------------- | --------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `x86_64-pc-windows-msvc`                      | `steam_api64.dll`           | `win64`                     | supported                                                                                                         |
| `x86_64-unknown-linux-gnu`                    | `libsteam_api.so`           | `linux64`                   | supported                                                                                                         |
| `aarch64-apple-darwin`, `x86_64-apple-darwin` | `libsteam_api.dylib`        | `osx` (universal x64+arm64) | supported; one universal file serves both                                                                         |
| `aarch64-unknown-linux-gnu`                   | `libsteam_api.so`           | `linuxarm64` (SDK 1.63+)    | loader path listed, **unverified**: no CI target, no test machine                                                 |
| any 32-bit target                             | `steam_api.dll`, `linux32/` | —                           | **out of scope**: the workspace ships no 32-bit build; the crate has no items when `target_pointer_width != "64"` |
| `wasm32`, Android, anything else              | —                           | —                           | crate compiles to its documentation and no public items                                                           |

Per-OS loading detail:

- **Windows:** `LoadLibraryExW(path, NULL, LOAD_WITH_ALTERED_SEARCH_PATH)` with
  the absolute path, so the DLL's own dependencies resolve from its directory
  and a planted `steam_api64.dll` in the current directory is never considered.
  `GetProcAddress` per symbol. The handle is leaked on purpose (below).
- **Linux:** `dlopen(path, RTLD_NOW | RTLD_LOCAL)`. Absolute because a bare
  `dlopen("libsteam_api.so")` searches `LD_LIBRARY_PATH` and the system, never
  the executable's directory. **The Steam Runtime:** a game Steam launches on
  Linux runs inside the Steam Linux Runtime — either the legacy `scout`
  `LD_LIBRARY_PATH` runtime or a `pressure-vessel` container (`sniper`, chosen
  per app on the partner site). The engine's other `dlopen`s (`libvulkan.so.1`,
  `libwayland-client.so.0`, `libxcb.so.1`) resolve inside the container, which
  provides them. The real constraint is glibc: a binary built on `ubuntu-latest`
  requires that runner's glibc, newer than the runtime's, and will not start
  inside `sniper`. Building shipping Linux binaries in the Steam Runtime SDK
  container is slice 15's job; development runs outside the container and is
  unaffected.
- **macOS:** `dlopen` of the universal dylib. For a signed, notarized `.app`,
  the dylib must be re-signed with the game's identity (library validation
  rejects a library signed by another team under the hardened runtime), and
  whether the Steam overlay's injection additionally needs the
  `com.apple.security.cs.allow-dyld-environment-variables` /
  `disable-library-validation` entitlements is **unverified** — a slice 15
  check. In development the dylib comes out of a browser-downloaded zip and may
  carry the `com.apple.quarantine` attribute; if `dlopen` refuses it, the
  `NoLibrary` message carries the loader's text, and the slice 1 manual step
  records whether clearing the attribute was needed.
- **How the overlay gets into the process differs by OS, and bears on every
  manual check.** Believed, not verified: on Windows the Steam client loads its
  overlay renderer into any process that initialises the API, so a
  `steam_appid.txt` development launch gets the overlay; on Linux and macOS
  Steam injects it at launch through `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES`, so
  a binary started from a terminal gets **no overlay** and
  `GameOverlayActivated_t` never fires. The slice 1b manual step establishes
  this per OS and records the launch that works (launched by Steam as a
  non-Steam shortcut, or with the preload set by hand). Until then, "overlay did
  not appear" on Linux or macOS is not evidence of a bug.

**Why the loaded module is never unloaded:** interface pointers and the callback
buffers Steam hands out point into it, so unloading while anything might hold
one is a use-after-free. `crcbl-shell` leaks its `dlopen` handles for the same
reason. The real `Lib` (module + resolved symbols) is loaded once and cached in
a `OnceLock<Result<&'static Lib, …>>` shared by `Steam::init` and
`Steam::relaunch_via_steam`; interface pointers are not cached there (see
"Ownership"). Tests never touch that cell — they build a fake `Lib`, leak it,
and hand it to a `#[cfg(test)]` constructor (see "The fake `Lib`").

**`steam_appid.txt` in development:** `SteamAPI_Init` reads the app id from
`steam_appid.txt` in the working directory when the process was not launched by
Steam. Default (user to confirm): **the crate never writes that file and never
sets `SteamAppId`** (`std::env::set_var` is `unsafe` in edition 2024 for good
reason — the audio and job threads are already running by the time a game
decides to init Steam). Instead, `InitError::NoSteamClient` carries Valve's
message plus the working directory and whether a `steam_appid.txt` was present
there, so the log line says what to do. Samples that opt into Steam document
`echo 480 > steam_appid.txt` beside their run command, and `.gitignore` keeps
the file out of history.

## The crate and its gating

A new crate, `crcbl-steam`, joining the workspace the standard way: globbed
member, pinned in `[workspace.dependencies]` with `path` **and** `version` (the
bare `{ path }` form is a wildcard to `cargo deny`), minimal manifest inheriting
workspace fields, `thiserror` and `log` as its only dependencies, and
`crcbl-net`, `crcbl-store` and `crcbl-input` added only by the slices that
implement their traits. **No JSON dependency, dev or otherwise.** The first
draft proposed `serde_json` as a dev-dependency for the drift gate; review found
the gate cannot rest on `steam_api.json` alone anyway (it omits the lifecycle
functions, `CallbackMsg_t` and every size), and the headers it must read instead
are line-structured C that a small scanner handles — see "The drift gate". If a
later need for parsing JSON appears, the workspace already has a hand-written
RFC 8259 parser (private, in `crates/crcbl-sprite/src/load.rs`); the move then
is to extract that, not to write a second or add `serde_json`.

Target gating follows the `crcbl-dx12` pattern — no `#![cfg(...)]` crate root:

- Every module that touches the FFI is
  `#[cfg(all(target_pointer_width = "64", any(target_os = "linux", target_os = "windows", target_os = "macos")))]`.
  Elsewhere the crate is its documentation and no public items. _Corrected in
  slice 1b:_ the first draft said nothing above the crate would ever ask
  `cfg(target_os)` about Steam, which cannot hold for a consumer that turns the
  feature on for a target with no items — and the workspace's `wasm32` clippy
  sweep builds every crate `--all-features`. `apps/sandbox/src/steam.rs` asks
  once, beside an inert stand-in; slice 8's `Loop` limb is where the question
  moves for games the loop hosts.
- Pure-logic modules — the callback-id table, payload decode, the call registry,
  the lobby/connection state machines, the voice resampling, the synced-file
  conflict rules — are additionally compiled under `test`, so they run on every
  CI host regardless.
- Items absent on some target are named in backticks in rustdoc, never
  intra-doc-linked, because `cargo doc` is a `-D warnings` gate on targets where
  they do not exist.
- **CI changes** (slice 1): the crate joins the "Type-check the platform
  backends as a consumer gets them" and "Document the platform backends on their
  own targets" steps in `.github/workflows/ci.yml`
  (`cargo clippy -p crcbl-steam --all-targets --no-default-features --target aarch64-apple-darwin`
  and `--target x86_64-pc-windows-msvc`, plus the matching `cargo doc` lines
  with `--document-private-items`), and the existing `test-cross-platform`
  matrix (macOS, Windows) plus the Linux jobs run its tests natively on all
  three OSes with no further change. A `miri (crcbl-steam)` job, modelled on
  `miri-jobs`, interprets the decode and fake-`Lib` tests (the escape-hatch
  rule: packed-struct reads and raw callback buffers are exactly what Miri is
  for).
- The umbrella exposes it (slice 1b) as `crates/crcbl/Cargo.toml`'s existing
  optional-dep shape: `steam = ["dep:crcbl-steam"]`, beside
  `scene = ["dep:crcbl-scene", …]`, and `pub use crcbl_steam as steam;` behind
  it.

## The binding route — decided

**Hand-written flat-API declarations + runtime loading** (ratified 2026-09-06).
The `crcbl-shell` pattern applied verbatim, laid out as:

- `crates/crcbl-steam/src/ffi/mod.rs` — scalar aliases (`HSteamPipe = i32`,
  `SteamAPICall = u64`, `AppId = u32`, …), and the `Lib` struct of typed
  function pointers, grouped per interface (`Lib.user`, `Lib.friends`, …) so a
  slice adds one group.
- the `prototype` aliases — one `type` per function pointer, each carrying in
  its doc comment **the C declaration it was copied from**. _Corrected in slice
  1:_ there is no `prototype.rs` file. The aliases are emitted by the
  `bindings!` macro invocation in `manifest.rs`, which is the only way they can
  be "generated from the same list" as the loads and the drift table.
- `crates/crcbl-steam/src/ffi/structs.rs` — the structs (callback payloads,
  `SteamNetworkingIdentity`, `InputAnalogActionData_t`, …) with the
  field-by-field layout table (below). Packing is chosen by one macro per
  pragma: callback-packed structs are
  `#[cfg_attr(windows, repr(C, packed(8)))] #[cfg_attr(not(windows), repr(C, packed(4)))]`
  (on x86-64 Windows `packed(8)` changes nothing from natural `repr(C)`, but
  spelling it keeps the "never reference a field" rule uniform on every OS);
  `pack(1)` structs are `repr(C, packed)`; structs declared outside any pragma
  (`SteamNetworkingMessage_t`) are plain `repr(C)`.
- `crates/crcbl-steam/src/ffi/versions.rs` — the accessor/interface-version
  table (`("SteamAPI_SteamUser_v023", "SteamUser023")`, …, both columns
  literal), the single source for accessor lookup and the init handshake string.
- `crates/crcbl-steam/src/ffi/load.rs` — the per-OS loader and the `symbol!`
  macro that resolves a name or returns `InitError::NoSymbol(name)`.
- `crates/crcbl-steam/src/ffi/manifest.rs` — a `const BINDINGS: &[BoundFn]`
  table in which each entry holds the function's name and **the C declaration it
  was copied from, verbatim**
  (`"S_API uint64_steamid SteamAPI_ISteamUser_GetSteamID( ISteamUser* self );"`),
  which is what the drift gate looks for in the headers. The symbol loads and
  the `prototype` aliases are generated from the same list by a declarative
  macro, so a function cannot be loaded without being in the manifest and the
  declaration is written once.
- `crates/crcbl-steam/src/ffi/drift.rs` — the drift gate, a `#[cfg(test)]`
  module inside the crate rather than a `tests/` file, because `BINDINGS` and
  the struct tables are private and an integration test cannot see them.

**Only what a slice uses is declared** — the flat header has on the order of a
thousand functions; each slice binds its dozens. Two checks keep the
declarations honest: the version handshake at init, and the drift gate.

**The layout tests** follow `crates/crcbl-shell/src/win32/ffi.rs`'s
`assert_layout!` exactly — size, then every field's offset and width, with a
destructuring pattern that makes a field without a row a compile error. Each
struct's table is per-OS where the packing differs (see the traps section), and
the numbers come from the SDK's own `sizeof`/`offsetof`, printed by a C program
compiled against the downloaded headers on each OS — the program is described in
`crates/crcbl-steam/src/ffi/structs.rs`'s docs, run locally, and its output
pasted as the table, the same provenance the Win32 table documents. Because the
`test-cross-platform` matrix runs natively (`windows-latest` x86-64,
`macos-latest` arm64) and the Linux jobs cover x86-64 Linux, the Windows
(`pack(8)`) and Linux/macOS (`pack(4)`) tables both execute in CI; macOS x86-64
shares the arm64 table and is not run. **The first table is Valve's own packing
sentinel**, `ValvePackingSentinel_t { uint32; uint64; uint16; double; }` from
`steamclientpublic.h`, which the header itself asserts is 24 bytes under
`VALVE_CALLBACK_PACK_SMALL` and 32 under `VALVE_CALLBACK_PACK_LARGE`: if the
packing macro picks the wrong arm on some target, that one test says so before
any real struct is read.

**The drift gate** (`crates/crcbl-steam/src/ffi/drift.rs`, `#[ignore]`d, run as
`cargo test -p crcbl-steam -- --ignored drift`): with `CRCBL_STEAM_SDK` set, it
reads the headers under `public/steam/` as text — no JSON — and asserts three
things:

1. **Every `BINDINGS` declaration appears in a header**, compared after
   collapsing runs of whitespace to one space: `steam_api_flat.h` holds one
   `S_API …;` prototype per line for every interface method, and the lifecycle
   functions are single-line `S_API … S_CALLTYPE …;` declarations in
   `steam_api.h`, `steam_api_internal.h` and `steam_gameserver.h`. An exact
   string match catches a changed type, an added parameter and a renamed
   function alike, and needs no C parser.
2. **Every `versions.rs` row matches**: the accessor name appears in
   `steam_api_flat.h`, and the version string is the one the interface header
   `#define`s as `STEAM…_INTERFACE_VERSION`.
3. **Every bound struct matches its header block**: from the `struct Name` line
   to the closing `};`, comments stripped, the `k_iCallback = base + n`
   expression (compared symbolically to the row in `callbacks.rs`) and the
   ordered field declarations; plus the `#pragma pack` in force at that line,
   compared to the struct's declared packing. That last check is what catches
   the `pack(1)` exceptions below if a later SDK moves a struct between blocks.

Without the variable it **fails** (it is only ever run on purpose, and "skipped"
must not read as "passed"). Proven red before trusted: change one parameter type
in a manifest declaration, change one field name, change one pragma — watch each
fail — and restore.

Rejected and not revisited: `steamworks-rs` (vendors Valve's SDK in the
published crate and links at build time — the dynamic-loader death above) and
build-time `bindgen` (every contributor and CI job would need libclang _and_ the
SDK to build the workspace).

## Ownership, threading, and what the types forbid

```rust
pub struct AppId(pub u32);

/// Everything Steam hands out that must outlive every user of it and then die
/// exactly once: the resolved interface pointers, the pipe, and the shutdown.
/// Never public. `Drop` runs `SteamAPI_Shutdown`.
struct Client { lib: &'static Lib, pipe: HSteamPipe, ifaces: Interfaces, /* … */ }

/// The pump owner: the event queue, the pending-call registry, the per-frame
/// entry points. `!Send + !Sync`: one owner, one pump thread.
pub struct Steam { client: Arc<Client>, queue: VecDeque<SteamEvent>, calls: CallRegistry, /* … */ }

impl Steam {
    /// Ships-through-Steam guard (`SteamAPI_RestartAppIfNecessary`). Loads the
    /// library into the same never-unloaded cache `init` uses — there is no
    /// "transient" load, because the module is never unloaded; `Ok(true)`
    /// means "quit now, Steam is relaunching us". Always `Ok(false)` while
    /// `steam_appid.txt` exists.
    pub fn relaunch_via_steam(app: AppId) -> Result<bool, InitError>;

    /// Load the library, `SteamInternal_SteamAPI_Init` with the bound version
    /// list, `SteamAPI_ManualDispatch_Init`, resolve and null-check every
    /// bound accessor. At most one live `Steam` per loaded library: a second
    /// call while one lives is `Err(InitError::AlreadyInitialised)`. A failed
    /// init never calls `SteamAPI_Shutdown`.
    pub fn init(app: AppId) -> Result<Steam, InitError>;

    /// `#[cfg(test)]` only: the same init sequence against a fake `Lib`.
    fn init_with(lib: &'static Lib, app: AppId) -> Result<Steam, InitError>;
}
```

**The "one live `Steam`" guard lives in the `Lib`, not in a process global** —
an `AtomicBool` field set by init and cleared by `Client`'s `Drop`. For the real
library that is the same thing as per-process, since there is one real `Lib`.
For tests it is what makes the rig work at all: `cargo test` runs a crate's unit
tests as threads of one process, so a process-wide flag would make every
fake-`Lib` test race every other for the right to exist. Each test leaks its own
fake and gets its own guard.

The lifetime rules are enforced by shape, not by comment:

- **Shutdown runs when the last owner is gone, and not before.** `Client` sits
  behind an `Arc`; `SteamAPI_Shutdown` is `Client`'s `Drop`. `Steam` holds one
  clone; the two surfaces that must satisfy existing `Send` traits —
  `SteamTransport` (`crcbl_net::Transport: Send`) and `SteamCloudStorage`
  (`crcbl_store::StorageSource: Send`) — hold others. So a transport outliving
  the pump owner keeps Steam alive rather than calling into a shut-down API, and
  there is no `shutdown(&mut self)` whose docs would have to say "don't use
  anything after this".
- **Accessor structs borrow `&Steam`** (`pub struct Friends<'a>(&'a Steam)`,
  obtained by `steam.friends()`), and every async token is redeemed through
  `&mut Steam`, so the borrow checker retires them before `Steam` drops.
- **Which surfaces are `Send`, and why that is sound: they are movable, but they
  only call Steam on the pump thread, and that is checked, not documented.** The
  first draft justified an `unsafe impl Send + Sync for Client` by Valve
  statements that turn out not to say what it claimed: the header's "you may
  call Release() from any thread" is about a received
  `SteamNetworkingMessage_t`, not the interface; `ISteamNetworkingSockets`'s
  partner page has no thread-safety statement at all; and the `steam_api.h`
  reference says only that `RunCallbacks` "is safe to call from multiple threads
  simultaneously" and that a single-threaded program need not call
  `ReleaseCurrentThreadMemory` — which implies multi-threaded use exists but
  promises nothing per interface. So the rule is enforced instead: `Client`
  records the pump thread's `ThreadId` at init, and every Steam call a `Send`
  surface makes goes through one `Client::on_pump_thread()` check first. Off
  that thread, `SteamTransport` returns `TransportError::Channel` naming the
  misuse and `SteamCloudStorage` returns `StorageError::Unsupported`, **without
  touching Steam**; a `Drop` off that thread skips its Steam call (the
  connection closes at `SteamAPI_Shutdown` instead) and logs. With that check,
  `unsafe impl Send + Sync for Client` is sound on grounds this crate controls:
  the pointers are only ever dereferenced on one thread. It costs nothing for
  the engine as it is — `crcbl-server` and `crcbl::session::Loopback` run on the
  caller's thread and spawn none. A game that genuinely wants its server on a
  worker thread gets a loud error, and the upgrade — forwarding calls to the
  pump thread through a queue, the browser storage backends' shape — is R1's
  fallback, built when someone needs it. Everything else is reachable only
  through `!Send` `Steam`.
- **Callbacks are never delivered to foreign-called Rust.** Manual dispatch is a
  poll; no `extern "C" fn` of ours is ever handed to Steam. Two APIs would hand
  one over — `SteamAPI_SetWarningMessageHook` and the networking config value
  `k_ESteamNetworkingConfig_Callback_ConnectionStatusChanged` — and both are
  deliberately not bound. If a later slice needs one, it must wrap the body in
  `catch_unwind` and say why polling will not do.
- **`crcbl_core::Handle<T>` is not used for Steam's ids.** Valve issues them,
  with Valve's semantics; a generation nothing checks would be a check that
  cannot fail. They are newtypes over the exact C width — `SteamId(u64)`,
  `LobbyId(u64)` (a `CSteamID` of lobby type), `AuthTicket(u32)`,
  `NetConnection(u32)`, `PublishedFileId(u64)`, `LeaderboardId(u64)`,
  `InventoryResult(i32)` — `0` or Valve's named invalid value meaning invalid.

### The fake `Lib`

`crates/crcbl-steam/src/testing.rs` (`#[cfg(test)]`) builds a `Lib` whose
function pointers are Rust `extern "C" fn`s reading a thread-local script (a
function pointer captures nothing, and each unit test runs on its own thread):
what `GetNextCallback` yields and in what order, what each accessor returns (a
non-null dangling sentinel, or null), what `SteamInternal_SteamAPI_Init`
answers, and counters for every call that matters (`FreeLastCallback`,
`SteamAPI_Shutdown`, `SteamNetworkingMessage_t_Release`, …). The pointers take
the exact types the `prototype` aliases declare, so the code under test calls
through the same signatures it calls the real library through. What the fake
**cannot** prove is that those signatures match C — a Rust callee compiled from
the same declaration agrees with it by construction. That is the drift gate's
job for types, and a real client's for calling convention and by-value returns.

### Shutdown order

What must happen, and what forces it to:

1. **The game tears down sessions** — drops each `SteamTransport` (its `Drop`
   closes the connection with linger, so a reliable goodbye flushes), drops its
   `Lobby` (its `Drop` calls `LeaveLobby`), drops any `VoiceCapture` (its `Drop`
   calls `StopVoiceRecording`), drops `AuthTicket`s (their `Drop` calls
   `CancelAuthTicket`). Each is RAII on the value that owns the resource, so a
   game cannot forget one; the order among them does not matter to Steam.
2. **`Steam` drops.** Pending `SteamCall` tokens are plain ids and die inert.
3. **The last `Arc<Client>` drops** → `SteamAPI_Shutdown`. If a `Send` surface
   is still alive, shutdown waits for it by construction. If that last drop
   happens off the pump thread, the shutdown call is skipped and logged, like
   every other off-thread Steam call — process exit then does what it would
   have.
4. The `Lib` stays mapped until process exit.

The game-server half (slice 13) is a sibling: its own `Arc<GameServerClient>`,
its own pipe, `SteamGameServer_Shutdown` in its `Drop`. A listen server that
also runs a game server (not EW) holds both; they do not nest.

### Error handling

House style — every variant a distinct, documented failure, `#[non_exhaustive]`:

```rust
pub enum InitError {
    NoLibrary { tried: Vec<PathBuf>, loader: String }, // every path + dlerror/GetLastError
    NoSymbol(&'static str),                            // SDK older than the declarations
    NoInterface(&'static str),                         // accessor returned null
    NoSteamClient { message: String, cwd: Option<PathBuf>, appid_file: bool },
    VersionMismatch(String),                           // Valve's SteamErrMsg
    Failed(String),                                    // FailedGeneric
    WrongApp { expected: AppId, running: AppId },         // slice 1: GetAppID disagreed
    AlreadyInitialised,
}

pub enum SteamError {
    InteriorNul(&'static str),        // which argument; never truncated
    Refused(&'static str),            // a bool-returning call said false; names it
    Result(EResult),                  // the call's own EResult, typed
    NotAvailable(&'static str),       // e.g. cloud disabled for account/app
}

pub enum CallError {
    IoFailure,                        // GetAPICallResult's pbFailed
    Decode { id: i32, expected: usize, got: usize },
}
```

Rules: a Steam `bool` failure becomes `Err(SteamError::Refused(name))` — never a
silently ignored `false`; strings Steam returns are copied to `String`
immediately (`from_utf8_lossy`, with a diagnostics counter on lossy); nothing in
the crate panics on Steam input.

## The pump — manual dispatch onto the frame loop

```rust
impl Steam {
    /// Drain the pipe: `SteamAPI_ManualDispatch_RunFrame`, then
    /// `ISteamInput::RunFrame(true)` when input is initialised (slice 7b), then
    /// the GetNextCallback / FreeLastCallback loop, then
    /// `SteamAPI_ReleaseCurrentThreadMemory`. Payloads decode to `SteamEvent`s;
    /// `SteamAPICallCompleted_t` routes to the call registry (slice 3a; in
    /// slice 1 every completion is unclaimed and counted); connection-status
    /// changes also update the shared networking state (slice 4). Once per
    /// frame.
    pub fn pump(&mut self);

    /// Drain the decoded events. The frame's idiom: pump, then drain, then act.
    pub fn events(&mut self) -> impl Iterator<Item = SteamEvent> + '_;

    /// Counters for the smoke gate: callbacks seen, unknown ids skipped, claimed
    /// ids whose size disagreed (must stay zero), lossy strings.
    pub fn diagnostics(&self) -> PumpDiagnostics;
}

#[non_exhaustive]
pub enum SteamEvent {
    OverlayActivated { active: bool },           // slice 1
    // …one variant per bound callback, added by the slice that binds it.
}
```

- **Why a drained queue and not closures or channels:** closures re-entering
  `&mut` engine state from inside `pump` are the borrow checker's least
  favourite shape; channels would add a thread and a `Send` bound nothing needs.
  The engine already has the idiom — `shell.pump(&mut |event| …)` once per frame
  (`Loop::frame_body` in `crates/crcbl/src/engine.rs`), and `crcbl-store`'s
  browser backends as "resident caches with an out-of-band fill".
- **Where it runs:** each app owns `Steam` and calls `pump`/`events` from its
  own frame until slice 8 gives the engine `Loop` a Steam limb (the standing
  rule: a helper is extracted at the second caller).
- **Async calls are typed tokens redeemed at the pump** (landed by slice 3a,
  with `CreateLobby` as its first caller — slice 1 binds no asynchronous call,
  so a registry there would be machinery with nothing to exercise it):

  ```rust
  #[must_use]
  pub struct SteamCall<T: CallResult>(ApiCall, PhantomData<fn() -> T>);

  pub enum CallState<T: CallResult> {
      Pending(SteamCall<T>),   // not answered; the token comes back
      Ready(T),
      Failed(CallError),
  }

  impl Steam {
      pub fn take<T: CallResult>(&mut self, call: SteamCall<T>) -> CallState<T>;
  }
  ```

  Registering in the pending set is the only way to construct a `SteamCall`. The
  entry records the expected callback id and size; `GetAPICallResult` is called
  with exactly those; a mismatch is `CallError::Decode`, not a reinterpret. A
  completion nobody registered means a dropped token (legal fire-and-forget) and
  is counted, not raised. Move semantics make double-redeem unrepresentable.

- **Unknown callback ids are skipped**, by design: the pipe carries dozens of
  ids we never bound and every SDK adds more. A _claimed_ id whose payload size
  disagrees with our struct is SDK drift and lands in
  `PumpDiagnostics::decode_mismatches`, which every smoke test asserts is zero.
- **Payload decode is packed `repr(C)` structs with the platform's packing,
  size-checked before read** — `m_cubParam` compared to `size_of`, a null
  `m_pubParam` refused, then one `read_unaligned` copy-out, fields only ever
  copied.
- **`k_iCallback` ids live in one table**
  (`crates/crcbl-steam/src/callbacks.rs`), each row `(id, name, decode fn)` —
  the size is `size_of` the declared struct, which the layout tables pin per OS
  — each id written as Valve's base plus offset
  (`K_I_STEAM_FRIENDS_CALLBACKS + 31`, which is 331 for
  `GameOverlayActivated_t`), and each checked by the drift gate against the
  header's own `k_iCallback` expression.

## Engine seams

Each Steam feature is a backend behind a seam the engine already has, or — where
the seam does not exist yet — the minimum seam this plan needs, defined so the
engine's own backends can share it. Nothing above a seam names Steam.

### Networking — `crcbl_net::Transport`

`crates/crcbl-net/src/transport.rs` defines `Transport: Send` — `send_reliable`,
`send_unreliable`, `recv_reliable`, `recv`, `is_connected` — message-oriented,
non-blocking, caller-driven, with
`TransportError::{Disconnected, Channel, MessageTooLarge, Backpressure}`.
`ISteamNetworkingSockets` has the same shape, and `FileTransport` in
`crates/crcbl-store/src/replay.rs` is precedent for a transport implemented
outside `crcbl-net`. So `SteamTransport` is one connection implementing
`Transport` (slice 4):

- `send_reliable` →
  `SendMessageToConnection(…, k_nSteamNetworkingSend_Reliable)`;
  `send_unreliable` → `k_nSteamNetworkingSend_Unreliable` (plus `NoNagle` for
  snapshots). A payload over `k_cbMaxSteamNetworkingSocketsMessageSizeSend` (512
  KiB in the SDK header) is `MessageTooLarge` before the call;
  `k_EResultLimitExceeded` from the send queue is `Backpressure`.
- `recv_reliable`/`recv` drain `ReceiveMessagesOnConnection` into an internal
  two-lane queue, reliable first (the received message's
  `k_nSteamNetworkingSend_Reliable` bit is the lane), copying each payload to a
  `Vec<u8>` and calling `SteamAPI_SteamNetworkingMessage_t_Release` at once — no
  Steam-owned message escapes. Reading `m_pData`, `m_cbSize` and `m_nFlags`
  means dereferencing a Steam-owned `SteamNetworkingMessage_t *`, so that struct
  gets a layout table too; it is declared **after** `steamnetworkingtypes.h`
  pops its 4/8 pack, so it is natural `repr(C)` on every OS.
- `is_connected` reads `GetConnectionInfo`'s state directly, so it is true
  without the pump having run.
- **Peer identity without tickets.** A P2P connection's
  `SteamNetConnectionInfo_t::m_identityRemote` is certified by Steam's relay
  authentication, so the host learns each peer's `SteamId` from the connection
  itself — `SteamTransport::remote()` — which is EW requirement 6's remote half
  and the reason EW needs no auth tickets (requirement 2).
- **Host side** is `SteamListener` (`CreateListenSocketP2P` on a virtual port):
  incoming `SteamNetConnectionStatusChangedCallback_t` in state `Connecting` is
  routed by the pump into the listener's shared queue; the listener **admits
  only current members of its lobby** (and optionally a caller-supplied
  allow-list), calls `AcceptConnection`, and yields a `SteamTransport` per peer.
  Everyone else is closed with an app-range end reason. A random Steam user who
  learns the host's id cannot join.
- **End reasons carry host-leave vs network loss.** The host closes a peer with
  `k_ESteamNetConnectionEnd_App_Min + n` (the app range, 1000–1999) plus a debug
  string; `crcbl-steam` defines
  `EndReason::{HostLeft, Kicked, ServerFull, ShuttingDown}` in that range, and
  maps the SDK's `ClosedByPeer` / `ProblemDetectedLocally` into
  `TransportError::Disconnected` with the reason retrievable from
  `SteamTransport::end_reason()`. Host-left is terminal for EW (no host
  migration); problem-detected enters reconnect.
- **Reconnect** reuses what `crcbl-net` already has:
  `SessionState::Reconnecting` and `SessionConfig::reconnect_grace_period` in
  `crates/crcbl-net/src/session.rs`, and
  `Hello::session_token: Option<ResumeToken>` in
  `crates/crcbl-net/src/handshake.rs`. The client opens a new `SteamTransport`
  to the same host `SteamId` and presents its resume token; nothing
  Steam-specific is added to the handshake.
- **Relay:** `ISteamNetworkingUtils::InitRelayNetworkAccess` is called at
  `Steam::networking()` first use, and `GetRelayNetworkStatus` is exposed so a
  lobby screen can wait for "relay ready" instead of failing its first connect.
- **What is not Steam's job: the multi-session host.** `crcbl-server`'s
  `Server<T: Transport>` (`crates/crcbl-server/src/lib.rs`) owns **one**
  transport and one `SessionManager`. A host serving three remote peers plus its
  own local client needs N transports, N sessions, one world. EW decided
  2026-09-22 that this is scheduled **with** the Steam work, as slice 2, built
  right after slice 4 and before cloud, and that it is transport-generic — it
  takes `crcbl_net::Transport`, not `SteamTransport` — with N a parameter (EW
  uses 4: the host and three peers). Its scope and boundary are under "Slice 2".
  Until it lands, slice 4's end-to-end test is one host and one peer.

`ISteamNetworkingMessages` (connectionless, UDP-shaped) is catalogued but not
used by `Transport`; it lands on demand (see the catalogue) if a game wants
unconnected pings. The deprecated `ISteamNetworking` is never bound.

### Invitations — lobbies, rich presence, overlay

No engine seam exists for "a group of friends about to play", and none is
invented: slice 3a's `Lobby` is a Steam type the game's menu drives, and the
seam it feeds is the one above — a lobby's owner `SteamId` is what a joiner
connects `SteamTransport` to. The flow EW needs:

1. Host: `matchmaking().create_lobby(LobbyKind::FriendsOnly, 4)` →
   `SteamCall<LobbyCreated>`; on ready, `SteamListener::open(lobby)`; set rich
   presence `connect` to `+connect_lobby <id>` and `steam_display` for the
   friends list.
2. Host invites: `friends().open_invite_dialog(lobby)`
   (`ActivateGameOverlayInviteDialog`) or `lobby.invite(friend)`.
3. Friend accepts, game running:
   `SteamEvent::LobbyJoinRequested { lobby, friend }`
   (`GameLobbyJoinRequested_t`) or
   `RichPresenceJoinRequested { friend, connect }`
   (`GameRichPresenceJoinRequested_t`). Game not running: Steam launches it with
   `+connect_lobby <id>`, read via `apps().launch_command_line()`
   (`GetLaunchCommandLine`) and, for a launch while already running,
   `SteamEvent::NewLaunchParameters`.
4. Joiner: `matchmaking().join_lobby(id)` → `SteamCall<LobbyEntered>` → read
   `lobby.owner()` → `SteamTransport::connect(owner)`.
5. Host leaves: the joiner sees three signals, in decreasing order of authority
   — slice 2's transport-neutral "session ended: host left" control message if
   the host quit cleanly, the transport end reason `HostLeft`, and
   `SteamEvent::LobbyMemberChanged { member: owner, change: Left }`. Steam
   passes lobby ownership on automatically; EW treats the session as over.

### Voice — the game's mixer, not `crcbl-audio`'s playback

EW spatialises and filters voice itself, so `crcbl-steam` hands over PCM and
stops. `ISteamUser`'s voice calls, verified from the mirror: `GetVoice` returns
Steam's compressed voice bytes; `DecompressVoice` turns them into "raw
single-channel 16-bit PCM audio. The decoder supports any sample rate from 11025
to 48000". Slice 5 requests 48000, which is `crcbl_audio::INTERNAL_SAMPLE_RATE`
(`crates/crcbl-audio/src/lib.rs`), and converts `i16` to
`crcbl_audio::AudioSample` (`f32`) so a game feeding `crcbl-audio` does no
format work. The compressed packets travel over the game's own transport (an
unreliable message on the same `SteamTransport`), so decompression happens on
the receiver — any Steam client can decompress any packet. `crcbl-audio` changes
nothing: its `Voice::new(Vec<AudioSample>)` (`crates/crcbl-audio/src/mixer.rs`)
plays a finished buffer, and a streaming voice source with a jitter buffer is
topic 32's work, which a game not using its own mixer would need and EW does
not.

**Push-to-talk is the game's, but it has a tail.** Recording starts and stops
only when the game calls `StartVoiceRecording` / `StopVoiceRecording`; Steam
never records on its own and never plays anything back through this API. But
`isteamuser.h` is explicit that stopping is not instant: "Because people often
release push-to-talk keys early, the system will keep recording for a little bit
after this function is called. GetVoice() should continue to be called until it
returns k_eVoiceResultNotRecording". So a PTT release ends the stream a short,
Steam-chosen interval later, and `VoiceCapture` keeps polling until it sees
`NotRecording` rather than going quiet on the release frame.

### Cloud — `crcbl_store::StorageSource`

`crates/crcbl-store/src/lib.rs` defines `StorageSource: Send` with `read`,
`write`, `delete`, `exists`, `list`, and `StorageError` already carries
`Pending` and `Unsupported` for backends like this one.

**Can Auto-Cloud meet "a conflict surfaces to the game"? No.** Valve's
[Steam Cloud documentation](https://partner.steamgames.com/doc/features/cloud)
(read 2026-09-22): Auto-Cloud "will automatically sync the groups of files when
the application launches and exits", with no code in the game. A conflict — two
devices changed the file since the last sync — is resolved by the **Steam
client's own "Cloud Sync Conflict" dialog before the game starts**, where the
player picks a side by timestamp and size; the game is told nothing and simply
finds whichever file won on disk. That fails EW's requirement twice: the game
never learns there was a conflict, and the losing side is discarded by a click
the game did not mediate. The ratified default ("Auto-Cloud first") is therefore
**overridden for EW's requirement 5** — the `ISteamRemoteStorage` backend
becomes required, which is exactly the condition the ratification named ("only
if per-file control … is wanted"). Auto-Cloud stays documented as the zero-code
path for games without that requirement.

The API does not make Steam's launch-time dialog go away either — it may still
appear. So conflict detection is **ours**, a backend-neutral protocol in
`crcbl-store` that works whatever Steam did before launch (slice 6):

- The profile is written as a **synced file**: a fixed header (`magic`,
  `format`, `generation: u64`, `base_generation: u64`, `writer: u64` — the
  device's random id — and the payload's length and CRC-32) followed by the
  payload. The CRC is corruption detection, not security. The workspace already
  has two CRC-32s, both test-local copies (in `crcbl-sprite`'s and
  `crcbl-golden`'s tests); slice 6 does not add a third. It writes one shared
  implementation in `crcbl-store`, tests it against the standard check value
  (`"123456789"` → `0xCBF43926`), and points the two test copies at it. Written
  whole via `SteamCloudStorage::write` → `FileWrite`, inside
  `BeginFileWriteBatch`/`EndFileWriteBatch`.
- A **local shadow** records what this device last synced (`generation`, CRC)
  and whether it holds an unsynced local write. It lives in
  `NativeStorage::data(app)` — not in Steam's remote directory — so it is never
  synced. (With Auto-Cloud this separation needs careful root-path
  configuration; with the API it is automatic.)
- **On load**: cloud generation equals the shadow's → clean. Cloud is newer and
  the shadow has no unsynced write → fast-forward. Cloud's `base_generation` is
  not the shadow's generation _and_ the shadow has an unsynced write (or the
  CRCs disagree at equal generation) → `SyncOutcome::Conflict { local, remote }`
  handed to the game with both payloads. The game picks one; the write that
  resolves it carries `generation = max + 1`. **Never merged, never partially
  applied**: the payload is opaque bytes to the protocol.
- **Mid-session changes** (Steam Deck suspend/resume, "Dynamic Cloud Sync"):
  `RemoteStorageLocalFileChange_t` →
  `GetLocalFileChangeCount`/`GetLocalFileChange` →
  `SteamEvent::CloudFileChanged { path }`, and the game re-runs the load check,
  which classifies the change exactly as at startup.
- Where it lives: the header, shadow and classification in `crcbl-store` (a
  `crcbl_store::synced` module, working over any `StorageSource` — so it is
  testable with `NativeStorage` and the in-memory test backend, and a future
  non-Steam cloud reuses it); the Steam half is only
  `SteamCloudStorage: StorageSource` plus the change event.
  `IsCloudEnabledForAccount` / `IsCloudEnabledForApp` false →
  `StorageError::Unsupported`, which the game treats as local-only.

### Input — the gamepad seam `crcbl-input` does not have yet

EW requirement 4: Steam Input must arrive as the **same** device events as the
evdev/XInput backends. Today there are none: `crates/crcbl-input/src/device.rs`
says of `Device::Gamepad` that "Nothing reports one yet: there is no gamepad
backend and no gamepad binding", and `Binding` (`crates/crcbl-input/src/lib.rs`)
has no gamepad member. The gamepad backends are topic 19's (`19-input.md`, evdev
P10, XInput/GameController P14) and the backlog's "P3: gamepad backends".

So Steam Input depends on a **gamepad seam**, specified here as the minimum
evdev, XInput, GameController and Steam Input all need. **Slice 7a lands it** in
`crcbl-input`, with no Steam code, unless topic 19's evdev slice has already
done so — EW confirmed 2026-09-22 that a minimal seam landed by this topic is
fine, and restated the requirement it must meet: every backend emits the same
events, so a game binds once. Topic 19's backends then adopt it rather than
define their own:

```rust
// crcbl-input
pub struct GamepadId(pub u32);                      // allocated by the input layer, per physical pad
pub enum PadButton { South, East, West, North, LeftShoulder, RightShoulder,
                     LeftStick, RightStick, Start, Select, DpadUp, DpadDown,
                     DpadLeft, DpadRight, Guide }     // positional, per 19-input.md
pub enum PadAxis { LeftX, LeftY, RightX, RightY, LeftTrigger, RightTrigger }

/// Backend-neutral: what one pad is doing now. A level, not a delta, like
/// `virtual_stick`.
pub struct GamepadSnapshot { pub buttons: PadButtons /* bitset */, pub axes: [f32; 6],
                             pub kind: PadKind /* Xbox, PlayStation, Switch, SteamDeck, Generic */ }

pub enum GamepadEvent { Connected { id: GamepadId, kind: PadKind },
                        Disconnected { id: GamepadId },
                        State { id: GamepadId, snapshot: GamepadSnapshot } }

impl ActionMap {
    /// Every backend — evdev, XInput, GameController, Web Gamepad, Steam Input —
    /// calls this and nothing else. Sets `last_device` to `Device::Gamepad` on
    /// activity, by the rules `device.rs` already states for the others.
    pub fn gamepad_event(&mut self, event: &GamepadEvent);
}

pub enum Stick { Left, Right }
pub enum Trigger { Left, Right }
pub enum Binding { /* … */ PadButton(PadButton), PadStick { stick: Stick, deadzone: f32 },
                   PadTrigger { trigger: Trigger, threshold: f32 } }
```

Two rules the seam states, because a backend cannot get them right on its own:
**axis conventions** — sticks −1…1 with +X right and +Y up (the convention
`ActionMap::virtual_stick` and `Binding::PointerPosition` already use), triggers
0…1; and **focus loss releases pads too** — `lose_focus` in
`crates/crcbl/src/engine.rs` releases held _keys_, and a pad held through an
overlay or alt-tab must resolve to released the same way, so the seam gives
`ActionMap` a way to drop every pad's level to neutral that `lose_focus`'s
callers invoke beside it.

**How Steam Input produces a neutral snapshot:** Steam Input is an action
mapper, and the way to make it emit a _pad_ rather than game actions is an
**action manifest whose one action set is the neutral pad itself** — a digital
action per `PadButton` and analog actions `left_stick`, `right_stick`
(`joystick_move`), `left_trigger`, `right_trigger`. Steam's configurator then
maps any physical controller (Deck, DualSense, Switch Pro, Steam Controller)
onto our positional layout, player remaps happen in Steam's UI, and every frame
`crcbl-steam` reads the actions per `InputHandle_t` into a `GamepadSnapshot` and
emits `GamepadEvent`s — the same values an evdev backend would produce. The
manifest (`crcbl-steam/assets/crcbl_pad.vdf` plus the per-controller default
configs Valve's format needs) ships with the game and is registered in
development with `SetInputActionManifestFilePath`, which needs no partner-site
configuration — so it is **believed** to work under app 480; whether Steam
honours a manifest path for an app whose own Steam Input configuration is
SpaceWar's is slice 7b's first manual check. The binding details the header
fixes: `Init(bExplicitlyCallRunFrame = true)` so the pump owns `RunFrame`;
`RunFrame( bool bReservedValue )` takes a bool in the flat API and "must be
called from somewhere before GetConnectedControllers will return any handles";
`SteamInputDeviceConnected_t` (2801) and `SteamInputDeviceDisconnected_t` (2802)
arrive only after `EnableDeviceCallbacks()`; and `EnableActionEventCallbacks`
takes a function pointer, so it is not bound (per "Callbacks are never delivered
to foreign-called Rust") — action data is polled.

**Double input must be impossible.** With Steam Input active for the app, Steam
also exposes a virtual XInput/evdev pad (Valve's USB vendor `0x28DE`). The rule
the seam carries: one backend owns a physical pad. When the Steam Input backend
is live, the evdev/XInput backends skip Valve's virtual devices, and the Steam
Input backend reports the pad; when Steam is absent, the native backends see the
real device. Slice 7b adds that filter to whichever native backend exists, with
a test.

Slice 7c adds the Deck-shaped `ISteamUtils` calls: `ShowGamepadTextInput` /
`ShowFloatingGamepadTextInput` and their dismissed callbacks, whose entered text
is delivered as the same committed text the shell's `ShellEvent::TextCommit`
(`crates/crcbl-shell/src/event.rs`) carries, so a text field cannot tell the
Deck keyboard from a physical one; and glyphs via `GetGlyphPNGForActionOrigin`,
exposed as a path the UI's glyph hints can load.

### The overlay, focus and pause

The overlay needs nothing wrapped — it injects into the present path. The engine
obligation is pause: `SteamEvent::OverlayActivated { active: true }` must pause
and release held input exactly as focus loss does. The engine already has that
function — `lose_focus` in `crates/crcbl/src/engine.rs`, which releases held
keys as real release events and sets `paused` — so slice 8's `Loop` limb routes
`OverlayActivated { active: true }` through it. _Corrected in slice 1b:_ the
first draft said that until then each app calls `lose_focus` itself, but a game
the `Loop` hosts cannot — the held keys and the pause are the loop's. So slice
1b gave `HostedGame` a `take_pending_focus_loss` hook (default `false`) that the
loop folds into its own focus-loss path; a game that pumps Steam reports an
opened overlay through it, and slice 8 can keep the hook as the seam its limb
feeds. **Whether the overlay composites over `crcbl-shell`'s own
Wayland/X11/Win32/AppKit windows and each GPU backend's swapchain is
unverified**, and stays a named line in every slice's manual checklist.

## Interface catalogue

Every client-side Steamworks interface in the current SDK, with the slice that
lands it or the reason it does not. "EW" marks what the first consumer needs.

| Interface / area                                                                          | What is bound                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Slice                                                                                                                                                                                                                                                                                                                                                     | EW                              |
| ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| Lifecycle                                                                                 | `SteamInternal_SteamAPI_Init`, `SteamAPI_Shutdown`, `SteamAPI_GetHSteamPipe`, `SteamAPI_ReleaseCurrentThreadMemory`, manual dispatch (`Init`, `RunFrame`, `GetNextCallback`, `FreeLastCallback`), `SteamAPI_IsSteamRunning`; then `SteamAPI_RestartAppIfNecessary`                                                                                                                                                                                                              | 1; 1b (restart guard)                                                                                                                                                                                                                                                                                                                                     | yes                             |
| Call results                                                                              | `SteamAPICallCompleted_t` (703) + `SteamAPI_ManualDispatch_GetAPICallResult`, `ISteamUtils::IsAPICallCompleted`/`GetAPICallFailureReason`. Slice 1 decodes the completion and counts it as unclaimed; the registry and `SteamCall<T>` arrive with the first async call                                                                                                                                                                                                          | 1 (decode only), 3a (registry)                                                                                                                                                                                                                                                                                                                            | yes                             |
| `SteamAPI_RunCallbacks`, `SteamAPI_InitFlat`                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | never — excluded by manual dispatch / no version check                                                                                                                                                                                                                                                                                                    | —                               |
| `ISteamUser` identity                                                                     | `GetSteamID`, `BLoggedOn`; then `GetPlayerSteamLevel`                                                                                                                                                                                                                                                                                                                                                                                                                           | 1; 1b                                                                                                                                                                                                                                                                                                                                                     | yes (6)                         |
| `ISteamUtils` basics                                                                      | `GetAppID`, `IsRunningOnSteamHardware` (replaced `IsSteamRunningOnSteamDeck`, which `SteamUtils011` no longer has); then `GetSteamUILanguage`, `IsOverlayEnabled`, `IsSteamInBigPictureMode`, `IsRunningUnderProton`, `GetSteamHardwareDefaultConfig`, `SetOverlayNotificationPosition`/`Inset`, `GetServerRealTime`, `GetIPCountry`                                                                                                                                            | 1; 1b                                                                                                                                                                                                                                                                                                                                                     | yes                             |
| `ISteamFriends` overlay event                                                             | `GameOverlayActivated_t` (331 — an `isteamfriends.h` callback, not a utils one)                                                                                                                                                                                                                                                                                                                                                                                                 | 1                                                                                                                                                                                                                                                                                                                                                         | yes                             |
| `ISteamApps` basics                                                                       | `BIsSubscribed`, `GetCurrentGameLanguage`; `GetLaunchCommandLine`, `NewUrlLaunchParameters_t`                                                                                                                                                                                                                                                                                                                                                                                   | 1b (basics), 3a (launch line)                                                                                                                                                                                                                                                                                                                             | yes                             |
| `ISteamFriends`                                                                           | 3a: `SetRichPresence`/`ClearRichPresence`, `ActivateGameOverlayInviteDialog`, `InviteUserToGame`, `GameLobbyJoinRequested_t`/`GameRichPresenceJoinRequested_t`. 3b: persona (own, friends', `PersonaStateChange_t`), friends list, avatars (+ `ISteamUtils::GetImageSize`/`GetImageRGBA`, `AvatarImageLoaded_t`), reading friends' rich presence, the other overlay dialogs (`ActivateGameOverlay`, `…ToUser`, `…ToWebPage`)                                                    | 3a, 3b                                                                                                                                                                                                                                                                                                                                                    | invites (1) in 3a; wanted in 3b |
| `ISteamMatchmaking` (lobbies)                                                             | create/join/leave, members, owner, data, member data, chat-update/data-update/enter callbacks, lobby chat messages                                                                                                                                                                                                                                                                                                                                                              | 3a                                                                                                                                                                                                                                                                                                                                                        | yes (1)                         |
| `ISteamNetworkingSockets`                                                                 | P2P listen/connect/accept/close, send/receive, connection info and real-time status, poll groups, status-changed callback                                                                                                                                                                                                                                                                                                                                                       | 4                                                                                                                                                                                                                                                                                                                                                         | yes (1)                         |
| `ISteamNetworkingUtils`                                                                   | `InitRelayNetworkAccess`, `GetRelayNetworkStatus`, `SteamRelayNetworkStatus_t`, ping location (for "region" display)                                                                                                                                                                                                                                                                                                                                                            | 4                                                                                                                                                                                                                                                                                                                                                         | yes (1)                         |
| `ISteamUser` voice                                                                        | `StartVoiceRecording`, `StopVoiceRecording`, `GetAvailableVoice`, `GetVoice`, `DecompressVoice`, `GetVoiceOptimalSampleRate`                                                                                                                                                                                                                                                                                                                                                    | 5                                                                                                                                                                                                                                                                                                                                                         | yes (3)                         |
| `ISteamRemoteStorage`                                                                     | `FileWrite`, `FileRead`, `FileExists`, `FileDelete`, `GetFileSize`, `GetFileTimestamp`, `GetFileCount`/`GetFileNameAndSize`, quota, `IsCloudEnabledForAccount`/`ForApp`, `SetCloudEnabledForApp`, write batches, local-file-change                                                                                                                                                                                                                                              | 6                                                                                                                                                                                                                                                                                                                                                         | yes (5)                         |
| `ISteamInput`                                                                             | `Init(true)`, `RunFrame(true)`, `Shutdown`, `SetInputActionManifestFilePath`, `EnableDeviceCallbacks`, controllers, action sets, digital/analog action data (returned by value, `pack(1)`), `GetInputTypeForHandle`, vibration/LED, `ShowBindingPanel`, `SteamInputDeviceConnected_t`/`Disconnected_t`; origins and glyphs in 7c. `EnableActionEventCallbacks` is never bound (it takes a function pointer)                                                                     | 7b, 7c (glyphs)                                                                                                                                                                                                                                                                                                                                           | yes (4)                         |
| `ISteamUtils` Deck text                                                                   | `ShowGamepadTextInput`, `GetEnteredGamepadTextInput`, `ShowFloatingGamepadTextInput`, `DismissFloatingGamepadTextInput`, `GamepadTextInputDismissed_t` (714), `FloatingGamepadTextInputDismissed_t` (738)                                                                                                                                                                                                                                                                       | 7c                                                                                                                                                                                                                                                                                                                                                        | yes (4)                         |
| `ISteamUserStats` achievements + stats                                                    | set/clear/get achievement, achievement display attributes and icon, int/float stats, `StoreStats`, `IndicateAchievementProgress`, global achievement percentages, `UserStatsReceived_t`/`Stored_t`/`UserAchievementStored_t`                                                                                                                                                                                                                                                    | 9                                                                                                                                                                                                                                                                                                                                                         | wanted                          |
| `ISteamUserStats` leaderboards                                                            | find/find-or-create, upload score (with details), download entries (global, around user, friends, users), attach UGC                                                                                                                                                                                                                                                                                                                                                            | 9                                                                                                                                                                                                                                                                                                                                                         | no                              |
| `ISteamScreenshots`                                                                       | `TriggerScreenshot`, `HookScreenshots` + `ScreenshotRequested_t`, `WriteScreenshot`, tag user/location                                                                                                                                                                                                                                                                                                                                                                          | 10                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamTimeline` (game recording)                                                         | tooltip, game mode, instantaneous/range events, game phases and their tags/attributes, "does recording exist" calls, open overlay to event/phase                                                                                                                                                                                                                                                                                                                                | 10                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamApps` rest                                                                         | DLC (`BIsDlcInstalled`, `GetDLCCount`, `BGetDLCDataByIndex`, `InstallDLC`/`UninstallDLC`, `DlcInstalled_t`), betas (`GetCurrentBetaName`, `GetNumBetas`/`GetBetaInfo`/`SetActiveBeta`, all in the 2026-09-22 mirror), ownership (`BIsSubscribedApp`, `BIsLowViolence`, `BIsVACBanned`, `GetEarliestPurchaseUnixTime`, `BIsSubscribedFromFreeWeekend`, `BIsSubscribedFromFamilySharing`, `GetAppOwner`), `GetAppInstallDir`, `GetAppBuildId`, `MarkContentCorrupt`, file details | 11                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamRemotePlay`                                                                        | session count/info, `BSendRemotePlayTogetherInvite`, session connected/disconnected callbacks                                                                                                                                                                                                                                                                                                                                                                                   | 11                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamUser` auth                                                                         | `GetAuthSessionTicket` (with `SteamNetworkingIdentity`), `GetAuthTicketForWebApi` + `GetTicketForWebApiResponse_t`, `BeginAuthSession`/`EndAuthSession` (peer-to-peer validation), `CancelAuthTicket`, `UserHasLicenseForApp`, `ValidateAuthTicketResponse_t`                                                                                                                                                                                                                   | 12                                                                                                                                                                                                                                                                                                                                                        | no (2)                          |
| Encrypted app tickets                                                                     | `RequestEncryptedAppTicket` → `EncryptedAppTicketResponse_t`, `GetEncryptedAppTicket`; server-side decryption via the separate `sdkencryptedappticket` library, loaded the same way                                                                                                                                                                                                                                                                                             | 12                                                                                                                                                                                                                                                                                                                                                        | no (2)                          |
| Game server                                                                               | `SteamInternal_GameServer_Init_V2`, `SteamGameServer_Shutdown`/`GetHSteamPipe`/`BSecure`/`GetSteamID`, `ISteamGameServer` (logon anonymous/token, server info, auth sessions, `UserHasLicenseForApp`, advertise), `ISteamGameServerStats`, game-server networking sockets, `ISteamMatchmakingServers` (server browser)                                                                                                                                                          | 13                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamUGC` (Workshop)                                                                    | query (all/user/details), subscribe/unsubscribe, item state and install info, download, create + update + submit, `ItemInstalled_t`, `DownloadItemResult_t`                                                                                                                                                                                                                                                                                                                     | 14                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamInventory`                                                                         | result handles, `GetAllItems`, `GetResultItems`, item definitions and properties, grant promo, consume, exchange, `StartPurchase`, prices                                                                                                                                                                                                                                                                                                                                       | 15                                                                                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamParties`                                                                           | beacons (advertise an open slot in the friends list)                                                                                                                                                                                                                                                                                                                                                                                                                            | on demand — lobbies + rich presence already cover EW's invite path                                                                                                                                                                                                                                                                                        | no                              |
| `ISteamNetworkingMessages`                                                                | connectionless send/receive                                                                                                                                                                                                                                                                                                                                                                                                                                                     | on demand                                                                                                                                                                                                                                                                                                                                                 | no                              |
| `ISteamParentalSettings`                                                                  | parental lock queries                                                                                                                                                                                                                                                                                                                                                                                                                                                           | on demand (a store requirement if the game has content restrictions)                                                                                                                                                                                                                                                                                      | no                              |
| `ISteamVideo`, `ISteamMusic`                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: broadcast/music-player control, no engine use                                                                                                                                                                                                                                                                                                   | no                              |
| `ISteamHTMLSurface`                                                                       | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: an embedded browser renderer, no engine UI can host it                                                                                                                                                                                                                                                                                          | no                              |
| `ISteamHTTP`                                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: no engine feature needs an HTTP client, and one that did would not tie it to Steam                                                                                                                                                                                                                                                              | no                              |
| `ISteamNetworking` (old P2P), `ISteamController`                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: deprecated by Valve in favour of `ISteamNetworkingSockets` and `ISteamInput`                                                                                                                                                                                                                                                                    | no                              |
| `ISteamMicroTransactions`                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | **declined** — there is no such client interface: microtransactions are the `ISteamMicroTxn` **Web API**, called from a server holding the publisher Web API key; the client only receives `MicroTxnAuthorizationResponse_t`. That is hosted infrastructure the project does not run. Reopen only with a backend; the one callback is trivial to add then | no                              |
| `ISteamAppList`, `ISteamMusicRemote`, `ISteamGameCoordinator`, `ISteamPS3OverlayRenderer` | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | removed from the SDK or not for PC games                                                                                                                                                                                                                                                                                                                  | no                              |

## Slice order

Each slice: scope, files, API sketch, tests that can fail, what CI verifies,
what needs a real Steam client (with app 480 on each OS), and exit criteria.
**Every slice** also runs the workspace gates
(`cargo clippy --all-targets -- -D warnings`, `cargo fmt --all`, `cargo test`,
plus clippy for `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` and
`x86_64-pc-windows-msvc` on `crcbl-steam`), re-runs the drift gate locally if it
touched `ffi/`, updates the `CHANGELOG.md` `[Unreleased]` section, deletes the
backlog lines it closes, and switches this document's crate-relative paths for
any file it creates to rooted ones.

**Manual verification, common to every slice:** Steam client running and logged
in; `CRCBL_STEAM_SDK` set; `steam_appid.txt` containing `480` in the working
directory; run on **Windows 11, a Linux desktop (and the Deck where the slice
says so), and macOS**; `cargo test -p crcbl-steam -- --ignored <slice-filter>`
plus the sample's manual steps; record per OS, in the slice's commit or backlog
entry, what was run and what was not. App 480 is shared by every Steamworks
developer: **assert mechanisms (init, events arriving, calls completing), never
values read back from 480.** Slices marked "two accounts" need two Steam
accounts on two machines (one may be a Deck). Overlay checks on Linux and macOS
use whatever launch slice 1b recorded as the one that injects the overlay (see
"Platforms").

### Build order

Slice numbers are identity; this is the sequence, set by EW's priority
(2026-09-22: "slices 1, 3, 4, then multi-session, then 6, 5 and 7; slice 9 is
optional; none of 10–15"):

1 → 1b → 3a → 3b → 4 → **2** (multi-session host) → 6 → 5 → 7a → 7b → 7c → 8 → 9
→ 10–15.

Slice 3b (persona, friends list, avatars) is EW-"wanted" rather than hard and
may trail 4 if EW asks; slice 8 lands once `apps/sandbox` and EW both pump
`Steam` by hand (the second-caller rule). 10–15 stay in scope for "the full
Steam API" and are built after everything EW uses.

### Status by slice

On branch `steam-sdk`, not merged to `main`:

- **Slice 1: done** (2026-09-22). CI-side tests green on Windows; `pack(4)`
  tables, the Linux/macOS loaders and the `miri (crcbl-steam)` job run only in
  CI. The drift gate has run (2026-09-23) against the Steamworks.NET mirror of
  the 1.65 headers (commit `ba71581f`, "Update to Steamworks 1.65[a]"), laid out
  as `$CRCBL_STEAM_SDK/public/steam/`: its first run found `GetAppID` declared
  as returning `AppId_t` where the header says `uint32` (the same ABI), and it
  passes since the fix. **Not run:** the drift gate against an SDK zip from
  Valve, `tests/smoke.rs`, and the manual steps below, on every OS.
- **Slice 1b: done** (2026-09-23). CI-side tests green on Windows (Miri clean,
  54 lib tests); the drift gate passes against the mirror with every new
  declaration and both new interface rows. Through the old Godot-bundled
  `steam_api64.dll` on the Windows machine the real loader resolved every
  lifecycle, `ISteamUser` and `ISteamFriends` symbol 1b added and stopped at
  `NoSymbol("SteamAPI_SteamApps_v009")` — that DLL predates `SteamApps009`.
  **Not run:** everything under "Needs a real client" below, on every OS — no
  1.65 redistributable was available, and a windowed sandbox run was not made on
  the shared development machine; `crcbl` and `sandbox` clippy for
  `x86_64-unknown-linux-gnu` (their `alsa-sys` build script needs a Linux
  sysroot; `crcbl-steam` itself was clippy'd for Linux, macOS and wasm32).
- **Slice 3a: next.**
- Slices 3b, 4, 2, 6, 5, 7a–7c, 8, 9, 10–15: not started.

**Slice 1 as built, where it differs from the text below**, each for a reason:

- **No `prototype.rs`.** The aliases come from the `bindings!` macro in
  `crates/crcbl-steam/src/ffi/manifest.rs`, beside the loads and the drift table
  (see "The binding route").
- **`Rc<Client>`, not `Arc`, and no pump-thread record yet.** Slice 1 has no
  `Send` surface. An `Arc` of a `!Send` type is what clippy's
  `arc_with_non_send_sync` rejects, and a thread id nothing reads would be dead
  code. Slice 4's `SteamTransport` brings the `Arc`, the `ThreadId`, the
  `on_pump_thread()` check and the `unsafe impl Send + Sync` together.
- **A successful init that fails later still shuts down.** "A failed init never
  calls `SteamAPI_Shutdown`" holds for a failure of
  `SteamInternal_SteamAPI_Init` itself. Once that succeeds, a null accessor or
  the wrong app drops the session, which calls `SteamAPI_Shutdown` exactly once,
  balancing the init (tested both ways).
- **`init(app)` checks the app.** `ISteamUtils::GetAppID` must equal `app`, or
  init fails with the new `InitError::WrongApp { expected, running }`. The
  typical cause is a stale `steam_appid.txt`. `NoSteamClient::cwd` is an
  `Option<PathBuf>`: the working directory can be unreadable, and reporting that
  is better than inventing one.
- **`SteamAPI_IsSteamRunning` is not bound.** Nothing in slice 1 calls it, and
  only what a slice uses is declared.
- **`PumpDiagnostics` counts null payloads separately** (`null_payloads`) from
  size mismatches. The lossy-string counter arrives with slice 1b's first string
  return.
- **No `test` arm on the target gate.** Every CI host that runs tests is a
  supported target, so compiling pure modules under `test` elsewhere would add
  no coverage.
- **The drift gate normalises spacing next to punctuation**, not just whitespace
  runs: the headers write both `ISteamUser* self` and `ISteamUser *self`. It
  finds each struct in whichever header defines it, rather than trusting a
  header name: the plan says `CallbackMsg_t` is in `steam_api_internal.h`, and
  it may be in `steam_api_common.h`.
- **Layout numbers come from a C++ program over the mirror's headers** (MinGW
  GCC; `pack(8)` as compiled on Windows, `pack(4)` by forcing
  `steamclientpublic.h`'s platform test in a copy), not over a Valve zip. Slice
  1's tables were first computed over this crate's own transcription and came
  out identical when re-derived from the headers.
- **`log` is not a dependency yet.** Slice 1 logs nothing, and `cargo machete`
  refuses an unused dependency.

**Slice 1b as built, where it differs from the text below:**

- **The overlay pause goes through a `HostedGame` hook, not a direct
  `lose_focus` call.** `apps/sandbox` is hosted by `crcbl::engine::Loop`, which
  owns the held keys and the pause, so it cannot call `lose_focus`. 1b added
  `HostedGame::take_pending_focus_loss` (default `false`), which the loop takes
  every frame and folds into the window's focus-loss path; the sandbox answers
  it from `SteamEvent::OverlayActivated { active: true }`. Tested in
  `crcbl::engine` (a held key is released to the game and the loop pauses; a
  second report while paused leaves it paused) and seen red with the wiring
  removed. `lose_focus`'s log line now says "input focus lost" rather than
  naming the window.
- **The sandbox pumps Steam from `HostedGame::draw`**, the one hook the loop
  calls every frame, paused or not; the overlay a frame sees reaches the loop on
  the next. It initialises Steam on windowed runs only — a `--headless` run is
  CI's and must not touch a developer's client — and `SteamLink` is inert
  without the feature or on a target with no Steam items (see "The crate and its
  gating").
- **`ISteamApps` joined the handshake too**, not only `ISteamFriends`:
  `BIsSubscribed` and `GetCurrentGameLanguage` need its accessor. The handshake
  is now `SteamUser023`, `SteamFriends018`, `STEAMAPPS_INTERFACE_VERSION009`,
  `SteamUtils011`, spelled out once in a test.
- **A null string counts as lossy.** No 1b binding is documented to return null;
  one that did reads as empty and lands in `PumpDiagnostics::lossy_strings`,
  like invalid UTF-8.
- **"A borrowed return fails" is enforced by the type, not a test.** Every
  string accessor returns `String`, so a borrow of Steam's buffer is not
  expressible. The test that stands in keeps the fake's one buffer, overwrites
  it in place after the call, and checks the copy survived; the lossy and null
  tests were each seen red.
- **`relaunch_via_steam`'s shared cache is tested at the cache.** Both entry
  points call `load::real`, which goes through `load::cached`; the test proves
  `cached` runs its loader once and keeps a failure. That `init` and
  `relaunch_via_steam` share `load::real` is by reading, not by test — there is
  no seam to count real loads through without loading the real library.
- **Named where the catalogue left names open:**
  `Utils::hardware_default_config` (`HardwareDefaultConfig`, with `Unknown(i32)`
  like `SteamHardware`), `Utils::set_notification_corner` (`NotificationCorner`,
  without Valve's `k_EPositionInvalid`), `Utils::server_unix_time`,
  `User::steam_level`.

### Slice 1 — Loader, init, pump, local `SteamId`

Deliberately small: the crate exists, loads the library on three OSes, inits
with the version handshake, drains the pipe correctly, shuts down exactly once,
and reads the local `SteamId`. No sample, no umbrella feature, no async calls.

- **Scope:** crate skeleton (`thiserror`, `log`) and workspace wiring; the
  loader on all three OSes (search order, absolute paths, `NoLibrary` listing
  every path); `Steam::init` with the version handshake and accessor null checks
  for the interfaces this slice binds (`ISteamUser`, `ISteamUtils`,
  `ISteamFriends` — the last only for `GameOverlayActivated_t`, which needs no
  accessor, so the handshake carries `SteamUtils011\0SteamUser023\0`); the
  per-`Lib` single-owner guard; `Client` + `Arc` shutdown with the pump-thread
  record; the manual-dispatch pump (drain, free-once, unknown-id skip, size
  check, `ReleaseCurrentThreadMemory`); `SteamAPICallCompleted_t` decoded and
  counted as unclaimed; `SteamEvent::OverlayActivated`; `user().steam_id()`,
  `user().logged_on()`, `utils().app_id()`, `utils().steam_hardware()`;
  `PumpDiagnostics`; the fake-`Lib` rig; the drift gate; CI steps; `.gitignore`
  lines; the `ROADMAP.md` phase claim.
- **Files:** `crates/crcbl-steam/Cargo.toml`, `crates/crcbl-steam/src/lib.rs`,
  `crates/crcbl-steam/src/ffi/mod.rs`, `crates/crcbl-steam/src/ffi/structs.rs`,
  `crates/crcbl-steam/src/ffi/versions.rs`,
  `crates/crcbl-steam/src/ffi/load.rs`,
  `crates/crcbl-steam/src/ffi/manifest.rs`,
  `crates/crcbl-steam/src/ffi/drift.rs`, `crates/crcbl-steam/src/client.rs`,
  `crates/crcbl-steam/src/pump.rs`, `crates/crcbl-steam/src/callbacks.rs`,
  `crates/crcbl-steam/src/error.rs`, `crates/crcbl-steam/src/user.rs`,
  `crates/crcbl-steam/src/utils.rs`, `crates/crcbl-steam/src/testing.rs`
  (`#[cfg(test)]` fake `Lib`), `crates/crcbl-steam/tests/smoke.rs` (`#[ignore]`,
  public API only); root `Cargo.toml` (workspace dependency pin),
  `.github/workflows/ci.yml`, `.gitignore`, `docs/plan/ROADMAP.md`.
- **API:**

  ```rust
  let mut steam = Steam::init(AppId(480))?;       // Err(InitError::…) on any machine without Steam
  let me: SteamId = steam.user().steam_id();      // EW 6: stable u64
  let hw: SteamHardware = steam.utils().steam_hardware(); // None | SteamDeck | SteamMachine | SteamFrame | Unknown(i32)
  steam.pump();
  for event in steam.events() { if let SteamEvent::OverlayActivated { active } = event { /* pause */ } }
  ```

  `SteamHardware` carries `Unknown(i32)` because Valve's own comment on
  `IsRunningOnSteamHardware` warns that future hardware will return values old
  SDKs do not name; an unnamed value must not be mistaken for `None`.

- **Tests that can fail (CI, all three OSes natively):**
  - Layout tables: `ValvePackingSentinel_t` (24 on Linux/macOS, 32 on Windows),
    `CallbackMsg_t` (20 / 24), `SteamAPICallCompleted_t` (16 on both),
    `GameOverlayActivated_t` (12 on both — a table that differs from nothing is
    fine as long as the sentinel and `CallbackMsg_t` do).
  - Drain protocol over the fake: an unknown id (skipped, counted); a claimed id
    with the wrong size (counted in `decode_mismatches`, not decoded); a null
    `m_pubParam` with a non-zero size (refused, counted); an unclaimed
    completion (counted); `GameOverlayActivated_t` decoding to
    `OverlayActivated { active }` for `m_bActive` 0 and 1; and
    **`FreeLastCallback` called exactly once per `GetNextCallback` true** (the
    fake counts; a loop that leaks or double-frees fails).
    `ReleaseCurrentThreadMemory` called once per `pump`.
  - Init: each `ESteamAPIInitResult` maps to its `InitError` variant with
    Valve's message copied; the handshake string equals the bound `versions.rs`
    rows joined with NULs and double-terminated, and contains no version absent
    from Valve's `InitEx` list; an accessor returning null is `NoInterface`
    naming it; a failed init never calls `SteamAPI_Shutdown` (fake counts); a
    second `init_with` on the same fake while one lives is `AlreadyInitialised`,
    and succeeds after the first drops; the last `Arc<Client>` drop calls
    `SteamAPI_Shutdown` exactly once, and not while a clone lives.
  - Loader: `NoLibrary` lists every path tried, in order, when pointed at an
    empty temporary directory; a missing symbol in a fake symbol table is
    `NoSymbol(name)`.
  - `SteamHardware` maps 0–3 and an out-of-range value to `Unknown`.

  Each broken once by hand before it is trusted (skip the free, swap the pack
  arms, drop the null check) and seen red.

- **CI verifies:** the above natively on Linux, Windows (x86-64) and macOS
  (arm64); `cargo clippy -p crcbl-steam --all-targets` and
  `cargo doc -p crcbl-steam --document-private-items` for `aarch64-apple-darwin`
  and `x86_64-pc-windows-msvc` in the existing "Type-check the platform backends
  as a consumer gets them" and "Document the platform backends on their own
  targets" steps; a `miri (crcbl-steam)` job modelled on `miri-jobs` running
  `cargo miri test -p crcbl-steam --lib` (decode, drain and layout tests; the
  loader test touches the filesystem and is `#[cfg_attr(miri, ignore)]`); the
  wasm job (the crate compiles to docs).
- **Needs a real client:** `smoke.rs` (`#[ignore]`): init succeeds under 480;
  `steam_id()` is non-zero and the same across two runs; 300 pumps with
  `decode_mismatches == 0`; drop then process exit is clean. The drift gate
  passes against the downloaded SDK and fails after each deliberate edit. Also,
  without a sample yet: run the smoke test with Steam **not** running →
  `NoSteamClient`, message says so; with the redistributable absent →
  `NoLibrary` with the paths; without `steam_appid.txt` → the message names the
  missing file and the working directory.
- **Exit:** CI green on all jobs; smoke + drift gate run and pass on all three
  OSes (or the OS not run is named as a gap in the backlog).

### Slice 1b — Umbrella feature, sandbox, remaining basics

- **Scope:** `crates/crcbl/Cargo.toml` gains `steam = ["dep:crcbl-steam"]`
  beside `scene = ["dep:crcbl-scene", …]`, and `pub use crcbl_steam as steam;`
  behind it; `apps/sandbox` behind its own `steam` feature calls init/pump, logs
  identity and overlay events, and pauses through the loop's focus-loss path on
  `OverlayActivated { active: true }` (as built: the
  `HostedGame::take_pending_focus_loss` hook — see "Slice 1b as built");
  `Steam::relaunch_via_steam`; the rest of the
  `ISteamUtils`/`ISteamApps`/`ISteamUser` basics in the catalogue
  (`friends().persona_name()` included, which adds `ISteamFriends`'s accessor
  and `SteamFriends018` to the handshake).
- **Files:** `crates/crcbl/Cargo.toml`, `crates/crcbl/src/lib.rs`,
  `crates/crcbl/src/engine.rs` (the hook), `apps/sandbox/Cargo.toml`,
  `apps/sandbox/src/app.rs`, `apps/sandbox/src/steam.rs`,
  `crates/crcbl-steam/src/apps.rs`, `crates/crcbl-steam/src/friends.rs` (only
  `persona_name` until 3b), `crates/crcbl-steam/src/strings.rs`, additions to
  `ffi/`.
- **Tests (CI):** `relaunch_via_steam` over the fake (`true`/`false`
  passthrough, library cached once across `relaunch_via_steam` then `init`);
  string-returning calls copy before returning (the fake reuses one buffer and
  overwrites it after the call — a borrowed return fails); lossy UTF-8 counted.
  The umbrella feature builds in the existing all-features CI jobs.
- **Needs a real client:** `apps/sandbox --features steam` on each OS: the
  overlay opens over the window (Shift+Tab) and the sandbox pauses and releases
  held keys; **record, per OS, which launch injected the overlay** (terminal,
  Steam non-Steam-game shortcut, or a hand-set preload) and whether a quarantine
  attribute had to be cleared on macOS; overlay composition per shell backend ×
  GPU backend tried; without Steam running the sandbox logs `NoSteamClient` and
  runs on.
- **Exit:** overlay pause observed on at least one OS, and each other OS either
  observed or recorded with the launch method that was tried.

### Slice 2 — Multi-session host (transport-generic)

Not Steam code, and not only for Steam: EW requirement 1 needs it whatever the
transport, and EW decided 2026-09-22 to schedule it with the Steam slices,
**built right after slice 4 and before slice 6**.

- **Boundary (EW's words, made concrete):** the engine owns the sessions — up to
  N peers, admission, per-peer resume, and telling disconnect from host-left —
  and delivers per-peer messages; the game owns authority and game state. N is a
  parameter (`HostConfig::max_peers`; EW passes 4, the host's own client
  included), never a constant.
- **Shape:** a new host type in `crcbl-server` beside `Server<T>` (which stays
  as it is — `crcbl::session::Loopback` and every single-peer caller keep
  working), holding one `SessionManager` and resume credential **per peer** and
  one world. Its peers' transports are `Box<dyn crcbl_net::Transport>` rather
  than one generic `T`, because a listen host is heterogeneous by nature: its
  own client arrives over `InMemoryTransport` (as `Loopback` wires it today) and
  the others over `SteamTransport` — `Transport` is object-safe (every method
  takes `&mut self` or `&self`, no generics).
- **Admission:** a new peer is admitted while fewer than N are connected; beyond
  that the handshake is rejected with a typed "full" reason. A peer presenting a
  valid `Hello::session_token` for a session in `SessionState::Reconnecting`
  within `SessionConfig::reconnect_grace_period` is re-attached to its own
  session (the per-peer form of `Server::reconnect`), not admitted as new, and
  does not count twice against N.
- **Host-left vs lost, transport-neutral:** `crcbl-net`'s messages have no
  goodbye today (`ServerToClient` is `Snapshot | Event`), so a peer can tell a
  dead link from a host that quit only through a transport-specific end reason.
  This slice adds a reliable control message — session ended, with a reason:
  host left, kicked, full, shutting down — sent before the host closes each
  transport, so any transport distinguishes the cases; Steam's app-range end
  codes (slice 4) become a second, redundant signal rather than the only one.
- **Files:** a new module in `crates/crcbl-server/src/` for the host (named in
  the slice), `crates/crcbl-net/src/messages.rs` for the control message, and
  their tests. Nothing in `crcbl-steam`.
- **Tests (CI), all over `InMemoryTransport`:** N peers connect and each gets
  its own snapshots; peer N+1 is refused with "full" and N=4 is not special (run
  with N = 2 and N = 4); one peer's link dropping puts only that session in
  `Reconnecting` while the others keep ticking; it resumes with its token inside
  the grace period and gets a fresh session after it; a token belonging to peer
  A presented by a new connection while A is still connected is rejected; host
  shutdown delivers "host left" to every peer before their transports report
  disconnected; a peer that sees only a dead transport reports "lost", not "host
  left". Each broken once by hand.
- **Needs a real client:** none of its own; slice 4's four-player run exercises
  it over Steam.
- **Exit:** tests green; slice 4's two-machine run repeated with three joiners
  (four accounts, or three plus the host) and recorded.

### Slice 3a — Lobbies, invites, join paths, the call registry

- **Scope:** the async call registry and `SteamCall<T>`/`CallState<T>` (first
  caller: `CreateLobby`); `ISteamMatchmaking` lobbies; the invite path
  (`ActivateGameOverlayInviteDialog`, `InviteUserToLobby`); the join paths
  (`GameLobbyJoinRequested_t`, `GameRichPresenceJoinRequested_t`, launch command
  line, `NewUrlLaunchParameters_t`); rich presence **set** (`connect`,
  `steam_display`). EW requirement 1's invitation path.
- **Files:** `crcbl-steam/src/{call,matchmaking,presence}.rs`, additions to
  `ffi/`, `callbacks.rs`, `friends.rs`, `apps.rs`; `apps/sandbox` lobby panel
  behind the feature.
- **API:**

  ```rust
  let call = steam.matchmaking().create_lobby(LobbyKind::FriendsOnly, 4)?;
  // later frame:
  if let CallState::Ready(created) = steam.take(call) { let lobby: Lobby = created.lobby()?; }
  steam.friends().open_invite_dialog(lobby.id());
  steam.friends().set_rich_presence("connect", &format!("+connect_lobby {}", lobby.id().0))?;
  match event { SteamEvent::LobbyJoinRequested { lobby, .. } => steam.matchmaking().join_lobby(lobby), … }
  let owner: SteamId = lobby.owner(&steam);
  ```

  `Lobby` is RAII (`Drop` → `LeaveLobby`), `!Send`, holds no Steam borrow (it
  takes `&Steam` per call) so a game can store it.

- **Tests (CI):** the registry over the fake — a registered call completes and
  decodes; a completion whose `m_iCallback` or `m_cubParam` disagrees with the
  registered expectation is `CallError::Decode` and `GetAPICallResult` is never
  called with the wrong size; `pbFailed` → `CallError::IoFailure`; `take` on an
  unanswered call hands the token back; a dropped token's completion is counted.
  Layout tables for `LobbyCreated_t` (513), `LobbyEnter_t` (504),
  `LobbyChatUpdate_t` (506), `LobbyDataUpdate_t` (505),
  `GameLobbyJoinRequested_t` (333), `GameRichPresenceJoinRequested_t` (337),
  `NewUrlLaunchParameters_t` (1014), each per OS, and decode of each from a byte
  fixture; `+connect_lobby` parsing from launch command lines (present, absent,
  malformed, trailing args) — pure; the lobby-member tracking state machine
  (enter, member joined/left/disconnected/kicked, owner change) from scripted
  callbacks; rich-presence key/value limits (`k_cchMaxRichPresenceKeyLength` =
  64, `k_cchMaxRichPresenceValueLength` = 256, `k_cchMaxRichPresenceKeys` = 30,
  all from `isteamfriends.h`) rejected before the call; `Lobby`'s `Drop` calls
  `LeaveLobby` once.
- **Needs a real client (two accounts):** A creates a friends-only lobby, B sees
  A as "in game" with rich presence; A invites via overlay; B accepts **with the
  game running** (join event) and **with it closed** (Steam launches with
  `+connect_lobby`); B's member list shows A as owner; A quits → B gets
  member-left for the owner. On each OS at least once as A and once as B.
- **Exit:** all four join paths demonstrated on at least two OSes and the rest
  named; decode mismatches zero throughout.

### Slice 3b — Persona, friends list, avatars, other overlay dialogs

- **Scope:** the rest of the `ISteamFriends` row: persona names and states
  (`PersonaStateChange_t`), the friends list, avatars through
  `ISteamUtils::GetImageSize`/`GetImageRGBA` and `AvatarImageLoaded_t`, reading
  friends' rich presence, `ActivateGameOverlay`/`…ToUser`/`…ToWebPage`. EW's
  "wanted" friends list.
- **Files:** `crcbl-steam/src/{friends,avatar}.rs`, additions to `ffi/` and
  `callbacks.rs`.
- **API:** `steam.friends().list(FriendFlags::IMMEDIATE) -> Vec<SteamId>`,
  `steam.friends().name(id) -> String`,
  `steam.friends().small_avatar(id) -> Option<Rgba>` (`None` until
  `AvatarImageLoaded`).
- **Tests (CI):** layout tables and fixture decode for `PersonaStateChange_t`
  (304) and `AvatarImageLoaded_t` (334); avatar RGBA buffer sized `4 × w × h`
  from `GetImageSize` and refused if Steam reports a size whose product
  overflows; a zero image handle is `None`, not a call.
- **Needs a real client:** friends list and avatars render in the sandbox panel;
  the overlay opens to a friend's profile and to a web page.
- **Exit:** observed on two OSes.

### Slice 4 — `SteamTransport` and `SteamListener` (P2P + relay)

- **Scope:** everything under "Networking" above. After slice 3a (admission
  reads lobby membership). Implements `crcbl_net::Transport`.
- **Files:**
  `crcbl-steam/src/net/{mod,transport,listener,identity,end_reason}.rs`;
  `crates/crcbl-steam/Cargo.toml` gains `crcbl-net`; a
  `crcbl-steam/tests/net_smoke.rs` (`#[ignore]`); `apps/sandbox` connects the
  lobby owner and exchanges a `crcbl-net` handshake. Plus, as its **own first
  commit**, a transport conformance suite in `crcbl-net` (below).
- **API:**

  ```rust
  let listener = SteamListener::open(&steam, &lobby, VirtualPort(0))?;   // host
  while let Some(peer) = listener.accept(&mut steam) { let who: SteamId = peer.remote(); host.add(Box::new(peer)); }
  let link = SteamTransport::connect(&steam, lobby.owner(&steam), VirtualPort(0))?; // joiner
  // both: impl crcbl_net::Transport for SteamTransport (Send; Steam calls on the pump thread only)
  link.end_reason() // Option<EndReason>: HostLeft | Kicked | ServerFull | ShuttingDown | Lost(code)
  ```

- **The conformance suite does not exist yet.** `crcbl-net`'s tests exercise
  `InMemoryTransport` directly; the only generic helpers are
  `drain_all<T: Transport>` (in `crates/crcbl-net/src/condition.rs` and
  `crates/crcbl-net/tests/replication.rs`). So the first commit extracts the
  behaviour `Transport`'s docs promise — reliable drained before unreliable,
  `recv_reliable` never returning unreliable traffic, `Message::kind` set by the
  send method, `MessageTooLarge` carrying size and limit, `is_connected` false
  after the peer drops — into functions generic over a `Transport` pair, run
  first against `InMemoryTransport` (which must pass unchanged, proving the
  suite) and then against `SteamTransport` on the fake loop. Where it lives (a
  `crcbl-net` module behind a test-support feature, or a helper crate) is the
  slice's call; it has exactly those two callers.
- **Tests (CI):** layout tables for `SteamNetworkingIdentity` (`pack(1)`,
  identical on every OS), `SteamNetConnectionInfo_t` and
  `SteamNetConnectionStatusChangedCallback_t` (1221; platform-packed — the
  `int64 m_nUserData` after the 136-byte identity sits at a different offset
  under `pack(4)` and `pack(8)`, so the tables must differ), and
  `SteamNetworkingMessage_t` (natural `repr(C)`); the transport over a fake
  `Lib` whose send/receive are an in-process loop: the conformance suite;
  `MessageTooLarge` at `k_cbMaxSteamNetworkingSocketsMessageSizeSend` + 1 and
  not at the limit; `Backpressure` on `k_EResultLimitExceeded`; every received
  message released exactly once (the fake counts); `is_connected` follows the
  fake's connection state; end-reason mapping for each SDK end code and each app
  code; listener admission: a non-member's `Connecting` is closed, a member's
  accepted — the admission test broken once (accept everyone) and seen red; a
  call from a second thread returns `TransportError::Channel` and makes **no**
  Steam call (the fake counts zero).
- **Needs a real client (two accounts, two machines, ideally two networks):**
  host + joiner connect through the lobby; handshake completes; 10 minutes of
  snapshots at the sample's tick rate with no disconnect; relay status reaches
  "current" before connect; pull the joiner's network for longer than a few
  seconds but under `reconnect_grace_period` → joiner reconnects and resumes
  with its `ResumeToken`; host quits → joiner sees `HostLeft`, not `Lost`; third
  account not in the lobby attempting `ConnectP2P` to the host is refused. Once
  across NAT (two homes, or a phone hotspot) to prove the relay path.
- **Exit:** conformance suite green on both transports; two-machine run on
  Windows↔Linux and Linux↔macOS at least. The four-player run belongs to slice
  2's exit.

### Slice 5 — Voice to PCM

- **Scope:** `ISteamUser` voice; push-to-talk under game control; PCM out. EW
  requirement 3.
- **Files:** `crcbl-steam/src/voice.rs`.
- **API:**

  ```rust
  let mut mic = steam.voice().capture()?;       // VoiceCapture: idle until transmitting; Drop stops
  mic.set_transmitting(ptt_held);               // game's push-to-talk: Start/StopVoiceRecording on edges
  while let Some(packet) = mic.poll(&steam)? { link.send_unreliable(Message::unreliable(packet.into_bytes()))?; }
  // receiver, per speaker:
  let pcm: Vec<crcbl_audio::AudioSample> = steam.voice().decompress(&bytes, SampleRate::INTERNAL)?; // mono f32 @ 48 kHz
  ```

  `poll` wraps `GetAvailableVoice` + `GetVoice(bWantCompressed = true, …)` with
  the **five** deprecated uncompressed arguments passed as `false`/null/zero,
  growing its buffer on `k_EVoiceResultBufferTooSmall`, and keeps polling after
  a PTT release until `k_EVoiceResultNotRecording` (the tail described under
  "Voice"); `decompress` wraps `DecompressVoice` at 48000 and retries once with
  the size Steam reports if the buffer was small.
  `EVoiceResult::{NotRecording, NoData, NotInitialized, RestrictedUser, …}` map
  to typed outcomes (`NoData` is `Ok(None)`, not an error).

- **Tests (CI):** `i16` → `f32` conversion endpoints (`i16::MIN` → `-1.0`, `0` →
  `0.0`, `i16::MAX` just under `1.0`) against known values; buffer-growth loop
  on a fake that answers `BufferTooSmall` then `OK` (and a fake that answers
  `BufferTooSmall` forever must terminate with an error, not spin);
  `EVoiceResult` mapping; PTT toggling calls start/stop exactly on edges; after
  a release, `poll` keeps returning packets the fake still has and stops only on
  `NotRecording`.
- **Needs a real client (two accounts):** A holds PTT and speaks; B's game
  receives packets over slice 4's transport, decompresses, and plays the PCM
  through `crcbl-audio` (a sample-side `Voice::new` per chunk is enough to hear
  it); releasing PTT ends the stream after Steam's short tail — record how long
  it was; confirm Steam itself plays nothing (mute the sample's output →
  silence). Per OS as speaker at least once.
- **Exit:** audible, intelligible round trip on two OSes; `RestrictedUser` path
  exercised or named as untested.

### Slice 6 — Cloud: `SteamCloudStorage` + synced-file conflicts

- **Scope:** everything under "Cloud" above. EW requirement 5.
- **Files:** `crates/crcbl-store/src/lib.rs` (module declaration) plus a new
  `crcbl-store/src/synced.rs` for the header, shadow and classification;
  `crcbl-steam/src/cloud.rs`; `crates/crcbl-steam/Cargo.toml` gains
  `crcbl-store`.
- **API:**

  ```rust
  // crcbl-store, backend-neutral
  let profile = SyncedFile::new(cloud: Box<dyn StorageSource>, shadow: &NativeStorage, "profile.bin");
  match profile.load()? {
      SyncOutcome::Clean(bytes) | SyncOutcome::FastForwarded(bytes) => …,
      SyncOutcome::Missing => …,
      SyncOutcome::Conflict { local, remote } => /* the game's UI picks */ profile.resolve(choice)?,
  }
  profile.save(&bytes)?;           // whole file, generation + 1, one FileWrite inside a batch
  // crcbl-steam
  let cloud = SteamCloudStorage::new(&steam)?;   // Err(Unsupported) when cloud is off for account/app
  SteamEvent::CloudFileChanged { path }          // Dynamic Cloud Sync → re-run profile.load()
  ```

- **Tests (CI):** the classification table as unit tests over
  `crcbl_store::MemoryStorage` — clean, fast-forward, missing, conflict by base
  mismatch, conflict by equal generation and different CRC, corrupt header
  (typed error, never treated as empty), truncated payload (CRC catches it) —
  each broken once by hand; resolving a conflict writes `max + 1` and a second
  load is clean; the shared CRC-32 against `"123456789"` → `0xCBF43926`;
  `SteamCloudStorage` path validation (relative, no `..`, at most
  `k_cchFilenameMax` = 260 from `isteamremotestorage.h`) and `Unsupported` when
  the fake reports cloud disabled; `FileWrite` returning false → `StorageError`,
  never `Ok`; `RemoteStorageLocalFileChange_t` (1333) layout and decode; a call
  from a second thread returns `Unsupported` and makes no Steam call.
- **Needs a real client:** does app 480 have a cloud quota? **Unverified** — the
  first step is `GetQuota`; if 480 has none, this slice's manual check waits for
  an app id of our own and says so. With quota: write on machine A, quit, launch
  on machine B → fast-forward; write offline on both, reconnect → the game shows
  `Conflict` (and record whether Steam's own dialog appeared first, and what the
  game saw after each choice); on a Deck, suspend mid-game, change the file from
  another machine, resume → `CloudFileChanged`.
- **Exit:** classification tests green; a real conflict surfaced to the game on
  at least one OS pair, or the 480-quota gap recorded.

### Slice 7a — The gamepad seam in `crcbl-input`

- **Scope:** the seam sketched under "Input", with no Steam code and no backend:
  `GamepadId`, `PadButton`, `PadAxis`, `Stick`, `Trigger`, `PadKind`,
  `GamepadSnapshot`, `GamepadEvent`, `ActionMap::gamepad_event`, the pad-neutral
  release beside `lose_focus`, and `Binding::PadButton`/`PadStick`/`PadTrigger`
  resolution. Skipped if topic 19 landed an equivalent first — then 7b adopts
  that. Its own reviewable commit (or commits).
- **Files:** `crcbl-input/src/gamepad.rs`, `crates/crcbl-input/src/lib.rs`
  (`Binding`, `ActionMap`), `crates/crcbl-input/src/device.rs` (the "Nothing
  reports one yet" doc becomes "every backend reports through `gamepad_event`"),
  and wherever `lose_focus`'s callers live in `crates/crcbl/src/engine.rs` for
  the pad release.
- **Tests (CI):** `PadButton` press/release edges through a `Binding`; stick
  deadzone and trigger threshold resolution against known values; +Y up;
  `last_device` becomes `Device::Gamepad` on activity and not on release, as the
  existing `device.rs` test states for the others; focus loss resolves a held
  pad button to released; a `Disconnected` pad's held state is released.
- **Exit:** tests green; `19-input.md` names this seam as the one its backends
  adopt (a one-line edit there).

### Slice 7b — Steam Input onto the seam

- **Scope:** `ISteamInput` init/run-frame/shutdown, the manifest, action-set
  activation, per-handle digital/analog reads into `GamepadSnapshot`, device
  callbacks, and the one-owner-per-pad filter. EW requirement 4.
- **Files:** `crcbl-steam/src/input.rs`, `crcbl-steam/assets/crcbl_pad.vdf` and
  the default controller configs; the vendor filter in whichever native backend
  exists (if none does yet, the filter's test lands with the first one, and this
  slice records that).
- **API:**

  ```rust
  let mut pads = steam.input().init(manifest_path)?;   // Init(true) + manifest + EnableDeviceCallbacks
  // each frame, after steam.pump() (which ran RunFrame(true)):
  for event in pads.poll(&steam) { action_map.gamepad_event(&event); } // crcbl_input::GamepadEvent
  ```

- **Tests (CI):** layout of `InputAnalogActionData_t` (13 bytes) and
  `InputDigitalActionData_t` (2 bytes), both `pack(1)` and identical on every
  OS; the by-value return path over the fake (plumbing only — see "The cases
  that are easy to get wrong" for why this cannot prove the ABI); action-data →
  snapshot mapping (stick Y normalised to +Y up — Steam's sign is pinned by the
  Deck run, not assumed — triggers 0..1, digital bitset) from fixtures;
  `SteamInputDeviceConnected_t` / `…Disconnected_t` → `GamepadId` stability for
  a re-plugged handle; **one owner per pad**: with the Steam backend live, a
  native-backend device with vendor `0x28DE` is skipped — test red when the
  filter is removed; `ActionMap` resolution of `PadButton`/`PadStick` bindings
  identical whether the event came from the Steam backend or a hand-built
  `GamepadEvent` (that identity is requirement 4, stated as a test).
- **Needs a real client:** first, whether the manifest path is honoured under
  480 at all. Then the Deck (Linux) as the primary: every `PadButton` and axis
  reaches the `ActionMap`, **which is also the by-value-return ABI check for
  x86-64 SysV**; remap in Steam's configurator and see the remapped action; no
  double input with a native backend present. Windows and macOS (arm64) with a
  DualSense and an Xbox pad: same checks — each OS run is that target's ABI
  check; confirm Steam Input is active for the app (the overlay's controller
  settings show the manifest's action set).
- **Exit:** Deck run recorded end to end; Windows and macOS runs recorded or
  named as gaps (and with them, the by-value return on that target named as
  unverified); the vendor filter test proven red once.

### Slice 7c — Deck text input and glyphs

- **Scope:** `ShowGamepadTextInput` / `ShowFloatingGamepadTextInput` and their
  dismissed callbacks, the entered text delivered as the same committed text
  `ShellEvent::TextCommit` carries; glyphs via `GetGlyphPNGForActionOrigin`,
  exposed as a path.
- **Files:** additions to `crcbl-steam/src/{utils,input}.rs`, `ffi/`,
  `callbacks.rs`.
- **API:** `steam.utils().show_text_input(TextInputRequest { … })?` →
  `SteamEvent::TextInputDismissed { text: Option<String> }`;
  `pads.glyph(&steam, id, PadButton::South) -> Option<PathBuf>`.
- **Tests (CI):** `GamepadTextInputDismissed_t` (714) and
  `FloatingGamepadTextInputDismissed_t` (738) layouts and decode; entered-text
  length from `GetEnteredGamepadTextLength` sizes the buffer, a submitted-false
  dismissal is `text: None`; glyph path copied before return.
- **Needs a real client:** the Deck keyboard fills a sandbox text field; glyphs
  load for a Deck and a DualSense.
- **Exit:** observed on the Deck; desktop Big Picture recorded or named.

### Slice 8 — The engine `Loop` limb

- **Scope:** `Loop` owns an `Option<Steam>` (umbrella `steam` feature), pumps it
  in `frame_body` under its own trace span beside `shell.pump`, routes
  `OverlayActivated { active: true }` through `lose_focus` (and slice 7a's pad
  release), and exposes the drained events to the game (a `HostedGame` hook with
  a no-op default). Lands once `apps/sandbox` and EW both pump by hand.
- **Files:** a new module under `crates/crcbl/src/` for the limb —
  `crates/crcbl/src/engine.rs` is recorded in `docs/backlog.md` as oversized, so
  this slice does not grow it beyond the call site — and
  `crates/crcbl/Cargo.toml`.
- **Tests (CI):** the loop with a scripted event source (the loop takes a trait
  object for "an event source it pumps", implemented by `Steam` and by a test
  double inside `crcbl`'s tests — the double is not a fake Steam and cannot sign
  anyone in): an overlay-open event pauses and releases held keys exactly as a
  focus loss does (the existing focus-loss test, parameterised over both
  causes); overlay-close does not unpause by itself (resume stays the player's
  action, matching focus).
- **Needs a real client:** Shift+Tab in a `Loop`-driven sample pauses it and
  releases held input on each OS (with slice 1b's recorded launch).
- **Exit:** samples stop calling `pump` themselves.

### Slice 9 — Achievements, stats, leaderboards

- **Scope:** `ISteamUserStats`, post-1.61 model (stats arrive without
  `RequestCurrentStats`; `UserStatsReceived_t` still signals readiness). First
  in-repo consumer: breakout's high score as a stat with an achievement.
- **Files:** `crcbl-steam/src/stats.rs`, `crcbl-steam/src/leaderboard.rs`.
- **API:** `stats.set_achievement("ACH_WIN_ONE_GAME")?`,
  `stats.set_i32("NumGames", n)?`, `stats.store()?` → `SteamEvent::StatsStored`,
  `stats.achievement("…")? -> Achieved { unlocked, unlock_time }`,
  `leaderboards.find_or_create(name, sort, display) -> SteamCall<Leaderboard>`,
  `upload(lb, method, score, &details) -> SteamCall<ScoreUploaded>`,
  `download(lb, Range::AroundUser(-5, 5)) -> SteamCall<Entries>`.
- **Tests (CI):** callback layouts; `Entries` decode through
  `GetDownloadedLeaderboardEntry` on a fake; details array length capped at
  `k_cLeaderboardDetailsMax` before the call; "store before stats received" is a
  typed error, not a silent false.
- **Needs a real client:** under 480, SpaceWar's own achievements and stats (the
  Steamworks example's `ACH_WIN_ONE_GAME`, `NumGames`, and its "Feet Traveled"
  leaderboard — **believed present, verify**): unlock → overlay toast appears;
  clear it again afterwards (480 is shared — leave it as found); upload a score
  and download around-user.
- **Exit:** toast observed on each OS; nothing asserted about 480's values.

### Slice 10 — Screenshots and the timeline (game recording)

- **Scope:** `ISteamScreenshots` (trigger, hook + write the engine's own capture
  of the swapchain image, tag), `ISteamTimeline` (events, game phases, overlay
  jumps).
- **API:** `screenshots.hook(true)` → `SteamEvent::ScreenshotRequested` → the
  game renders its capture → `screenshots.write(&rgb, w, h)`;
  `timeline.instant_event(TimelineEvent { title, description, icon, priority, offset, clip })`,
  `timeline.range_start(…) -> TimelineRange` (RAII end on drop),
  `timeline.set_game_mode(Playing | Staging | Menus | LoadingScreen)`.
- **Tests (CI):** RGB buffer size validation before `WriteScreenshot`; the
  timeline's float offsets rejected when non-finite; RAII range end called once.
- **Needs a real client:** F12 with the hook on produces the engine's image in
  the Steam screenshot manager; with game recording enabled in Steam, timeline
  events appear on the recording timeline.
- **Exit:** both observed on at least Windows and Linux.

### Slice 11 — Apps (DLC, betas, ownership) and Remote Play

- **Scope:** the rest of `ISteamApps`, `ISteamRemotePlay`.
- **Tests (CI):** DLC-by-index decode, string-buffer growth for
  `GetAppInstallDir` and beta names, `DlcInstalled_t` layout.
- **Needs a real client:** 480 owns no DLC — mechanism only (counts return,
  calls do not fail); Remote Play Together session detected when a friend joins
  via Steam's Remote Play invite.
- **Exit:** mechanisms observed; DLC semantics named as untestable under 480.

### Slice 12 — Auth session tickets and encrypted app tickets (client)

- **Scope:** `GetAuthSessionTicket` (with the identity argument), web-API
  tickets, peer validation (`BeginAuthSession` on a listen host, no game
  server), `CancelAuthTicket`/`EndAuthSession` as RAII; encrypted app tickets.
  Low priority: EW needs neither (requirement 2). Design with topic 27: an
  `auth_ticket` beside `Hello::session_token`, validated by a gate the server
  configures, admitting provisionally until `ValidateAuthTicketResponse_t`.
- **Tests (CI):** ticket buffer sizing; RAII cancel-once; the provisional-admit
  state machine (validated, rejected late, never answered within a timeout).
- **Needs a real client (two accounts):** A's ticket validates on B; a tampered
  ticket is rejected; cancelling A's ticket ends B's session.
- **Exit:** both outcomes observed.

### Slice 13 — Game-server API

- **Scope:** `SteamInternal_GameServer_Init_V2` (anonymous logon for dedicated
  servers), `ISteamGameServer`, game-server networking sockets, server-side
  auth-session validation, `ISteamGameServerStats`, `ISteamMatchmakingServers`.
  Whether it is a module of `crcbl-steam` or its own crate is decided here, when
  a dedicated headless build wants it. EW does not (listen server).
- **Tests (CI):** the sibling client's init/shutdown ordering on a fake; pipe
  separation (a callback on the game-server pipe never lands in the client
  queue).
- **Needs a real client:** a headless server on Linux logs on anonymously under
  480 and appears in the server browser query from a client.
- **Exit:** logon and browse observed.

### Slice 14 — Workshop (`ISteamUGC`)

- **Scope:** query, subscribe, download, install info, create/update/submit.
  Consumer: the editor/modding story (topics 8, 16).
- **Tests (CI):** query-handle RAII (`ReleaseQueryUGCRequest` once), paging,
  update-handle state machine, install-folder string growth.
- **Needs a real client:** under 480, upload a private test item, subscribe from
  a second account, see `ItemInstalled_t`, delete the item afterwards.
- **Exit:** round trip observed.

### Slice 15 — Inventory, then shipping

- **Inventory:** `ISteamInventory` result handles (RAII `DestroyResult`), item
  definitions, grant-promo/consume/exchange, purchase start. Under 480 the
  Steamworks example's item definitions — **believed present, verify**.
- **Shipping** (last, needs an app id of our own to mean anything): per-OS
  packaging that places the redistributable next to the executable (and in
  `Contents/Frameworks` for a macOS bundle, re-signed); a CI check that a
  packaged build contains no `steam_appid.txt`; Linux release builds in the
  Steam Runtime SDK container so the glibc floor matches `sniper`; the
  `relaunch_via_steam` guard in release builds; the overlay entitlement question
  on macOS answered.
- **Exit:** a depot-shaped build per OS that launches from Steam.

## What CI proves, what it cannot, and how the gap stays honest

- **CI proves:** the crate compiles on all four targets with all features; the
  layout tables for both packings, run natively on the OS each describes; the
  decode tables, call registry, pump drain protocol, transport conformance,
  voice conversion, cloud classification and input mapping — all over the fake
  `Lib`, which exercises the real code paths, not copies; Miri over the unsafe
  decode.
- **CI cannot prove:** that the declarations match a real SDK, that a real
  client accepts the handshake, that the overlay composites, that two peers
  connect through the relay, that voice is intelligible, that Steam Input maps a
  Deck. Each has a named home instead of a green light: the drift gate and the
  per-slice smoke tests are `#[ignore]`d and run locally, and a slice's backlog
  entry names which OS each was run on. Ignored is the honest verdict — "not run
  here" — never "passed".
- **A mock cdylib was considered and declined.** A fake `libsteam_api` built
  from the workspace would exercise `dlopen` too, but Cargo cannot build a
  cdylib as a test prerequisite without the unstable artifact-dependency
  feature, and a fake named like the real library could land beside a sample's
  executable in `target/` and be loaded by a developer's real run — a game
  silently "signed in" to nothing. The fake stays a struct of function pointers
  behind `#[cfg(test)]`: not a registered backend, not reachable from any
  feature, not constructible by a game. Games that want to run Steam-less hold
  `Option<Steam>` and got `None` from an ordinary `Err`. The loader itself is
  small and its failure path (`NoLibrary`) is tested by pointing it at an empty
  directory.

## The cases that are easy to get wrong

- **Callback packing differs by OS.** `steamclientpublic.h` defines
  `VALVE_CALLBACK_PACK_SMALL` on Linux, macOS and FreeBSD and
  `VALVE_CALLBACK_PACK_LARGE` elsewhere, and each header that declares callback
  structs (and `steam_api_internal.h`, for `CallbackMsg_t`) wraps them in
  `#pragma pack(push, 4)` or `pack(push, 8)` accordingly. Tables must disagree
  across platforms for any struct with an 8-byte field at an offset that is 4
  mod 8, **or** whose unpadded size is 4 mod 8 and contains an 8-byte field or
  pointer (the trailing padding `pack(8)` adds) — `CallbackMsg_t` is the second
  kind. If every table agrees, suspect the table; `ValvePackingSentinel_t` is
  the canary.
- **And some structs ignore that rule.** `steamnetworkingtypes.h` opens with the
  same 4/8 selection but wraps `SteamNetworkingIPAddr` and
  `SteamNetworkingIdentity` in `#pragma pack(push,1)`; `isteaminput.h` wraps
  `InputAnalogActionData_t` and `InputDigitalActionData_t` in
  `#pragma pack( push, 1 )`. Read the pragma in force at each struct, never
  assume the file's.
- **Struct returned by value from a packed type.** `GetAnalogActionData` returns
  `InputAnalogActionData_t` (`EInputSourceMode eMode; float x, y; bool bActive;`
  under `pack(1)`: 13 bytes, alignment 1) by value, and `GetDigitalActionData`
  and `GetMotionData` do the same for their `pack(1)` structs; the flat API has
  no out-pointer alternative. Rust's `extern "C"` must return it the way the C
  compiler does. Reasoned from the ABIs, not verified: the fields happen to sit
  at their natural offsets (0, 4, 8, 12), so SysV x86-64 sees no unaligned field
  and returns it in two integer registers; Windows x64 returns anything not 1,
  2, 4 or 8 bytes through a hidden pointer; AArch64 returns a composite of at
  most 16 bytes in `x0`/`x1`. `#[repr(C, packed)]` gives rustc the same size and
  alignment, so it should agree on each. **No CI test can confirm that**: a fake
  callee written in Rust from the same declaration agrees with the caller by
  construction, whatever C would do. The fake test proves the plumbing; slice
  7b's real-controller run on each OS/architecture is the ABI check, and a
  target not run is named unverified.
- **C++ references in the "flat" API.** `ConnectP2P`'s
  `const SteamNetworkingIdentity &` is a pointer at the ABI. Declared as
  `*const`, never by value.
- **Never reference a packed field.** Every access is a copy (`read_unaligned`,
  then moves). Miri runs over the decode tests.
- **`GetNextCallback` without `FreeLastCallback`** is a protocol violation; the
  drain loop owns the pair, decodes strictly between them, and the fake counts
  both.
- **Never bind `SteamAPI_RunCallbacks`.** Absence is the enforcement.
- **`SteamAPI_InitFlat` does not version-check.** Not bound.
- **Returned C strings are Steam's buffer**, valid until the next call. Copied
  to `String` before returning, always.
- **`&str` → C string needs a NUL check** — `InteriorNul`, never truncation.
- **Stale recall of signatures.** `GetAuthSessionTicket` grew a
  `const SteamNetworkingIdentity *` parameter; `GetVoice` still carries five
  deprecated uncompressed arguments (`bWantUncompressed_Deprecated` through
  `nUncompressedVoiceDesiredSampleRate_Deprecated`) — the first draft of this
  plan said four, which is exactly the mistake this bullet warns about;
  `ISteamUtils::IsSteamRunningOnSteamDeck` no longer exists in `SteamUtils011`.
  Bindings from memory corrupt the stack or fail to load. Every declaration
  carries its SDK version; the drift gate re-checks the lot.
- **Received networking messages must be released exactly once** — copied out
  and released immediately in `recv`.
- **Tickets, lobbies, recordings and connections leak** unless something ends
  them. RAII on the owning value; not `Drop` of `Steam`.
- **Steam Input's virtual pad doubles input** through the native backends unless
  filtered (slice 7b).
- **`steam_appid.txt` inverts a guard.** With it present,
  `RestartAppIfNecessary` returns false regardless — correct for dev,
  catastrophic if shipped. `.gitignore` plus slice 15's package check.
- **App id 480 is shared.** Mechanisms only; leave achievements as found.

## Risks and open questions

Each is labelled with what it blocks and whether it needs the user.

- **R1 — Thread safety of the `Send` surfaces (slices 4 and 6).** Valve states
  no thread-safety guarantee for `ISteamNetworkingSockets` or
  `ISteamRemoteStorage` (checked 2026-09-22: the partner pages and the 1.65
  headers; the "any thread" wording in `isteamnetworkingsockets.h` is about
  releasing a message). So neither surface calls Steam off the pump thread — the
  check under "Ownership" makes such a call a typed error with no Steam call
  made. What this blocks: a game that runs its server or its saves on a worker
  thread. The fallback, if one does: forward calls to the pump thread through a
  queue and answer `StorageError::Pending` / queue sends until the pump runs —
  the browser storage backends' shape. Not needed by EW (its host runs on the
  frame thread).
- **R2 — Steam's launch-time conflict dialog (slice 6).** It can still pre-empt
  the game; our protocol then sees the chosen file. What the game observes after
  each dialog choice is recorded in slice 6's manual run, not assumed.
- **R3 — App 480 capabilities (slices 6, 7b, 9, 14, 15).** Cloud quota, a Steam
  Input manifest path honoured under 480, SpaceWar's achievements, leaderboard
  and item definitions, and rich-presence localisation under 480 are believed,
  not verified. Each slice's first manual step checks, and a missing capability
  waits for our own app id rather than faking it.
- **R4 — Overlay over our own windowing (every slice).** Unverified on every
  shell backend × GPU backend; recorded per slice.
- **R5 — Overlay injection outside a Steam launch (slice 1b, then every overlay
  check).** Believed: Linux and macOS inject only at a Steam launch. Slice 1b
  records what works per OS.
- **R6 — Gamepad seam ownership (slice 7a).** Settled for this topic: EW
  accepted a minimal seam landed by slice 7a. Still open on topic 19's side —
  its evdev/XInput/GameController slices must adopt the seam rather than define
  their own, and `19-input.md` gets that line in slice 7a.
- **R7 — Linux shipping glibc floor (slice 15).** Binaries built on
  `ubuntu-latest` will not start inside `sniper`. Needs a container build.
- **R8 — macOS signing/entitlements for the dylib and overlay (slice 15).**
  Unverified.
- **R9 — Accessor versions move.** The table above is the 1.65 mirror's; the
  first real SDK may differ. The drift gate is the answer; until it has run
  once, nothing in `versions.rs` is trusted.
- **R10 — By-value struct returns (slice 7b).** The ABI for
  `InputAnalogActionData_t` and friends is reasoned, not tested; only real
  controllers on each OS/architecture confirm it.
- **Needs the user, not blocking any slice before 15: an app id of our own.**
  Achievement schemas, cloud quota, rich-presence tokens, Steam Input default
  configurations and depots are per-app. Until then 480.

Decided since the first draft, and no longer open: the multi-session server is
scheduled with the Steam slices as slice 2 (EW, 2026-09-22); the
`ISteamRemoteStorage` cloud backend is required (EW, same day); the drift gate
needs no JSON dependency (review).

## Defaulted decisions

The user was away; each of these was picked by this plan and is the user's to
overturn, except where the row says EW decided it. The four ratified 2026-09-06
are listed first for completeness.

| Decision                                                                | Default taken                                                                                                                                                                                              | Alternative, and its cost                                                                                                                       |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Binding route (ratified)                                                | hand-written flat-API declarations, drift gate                                                                                                                                                             | `steamworks-rs`: new dependency, link-time death without the library                                                                            |
| Cloud (ratified, then **overridden by EW**, confirmed by EW 2026-09-22) | `ISteamRemoteStorage` backend required, because Auto-Cloud cannot surface a conflict                                                                                                                       | Auto-Cloud only: zero code, but EW requirement 5 unmet                                                                                          |
| Steam Input (ratified "if Deck targeted" → now in scope)                | Steam Input onto a shared gamepad seam, one owner per pad                                                                                                                                                  | rely on Steam's XInput/evdev emulation only: no Steam code, but no Deck glyphs/remap awareness, and EW 4's "same events" only holds by accident |
| First app id (ratified)                                                 | 480 for every slice until slice 15                                                                                                                                                                         | own app id: partner fee and a product decision                                                                                                  |
| Multi-session server (**decided by EW** 2026-09-22)                     | slice 2, built after slice 4 and before cloud; transport-generic (`Box<dyn Transport>` peers); N a parameter; the engine owns sessions, admission, resume and host-left, the game owns authority and state | EW's host fans out over several transports itself: every co-op game re-writes session management                                                |
| Gamepad seam (**confirmed by EW** 2026-09-22)                           | slice 7a lands a minimal seam in `crcbl-input` unless topic 19 already has                                                                                                                                 | wait for topic 19: EW requirement 4 waits too                                                                                                   |
| Slice order (**set by EW** 2026-09-22)                                  | 1 → 1b → 3a → 3b → 4 → 2 → 6 → 5 → 7a–c → 8 → 9 → 10–15                                                                                                                                                    | the earlier order (stats before networking): EW waits                                                                                           |
| Loading                                                                 | runtime `dlopen`/`LoadLibraryExW`, absolute paths, exe dir then `CRCBL_STEAM_SDK`                                                                                                                          | link-time: CI cannot build                                                                                                                      |
| SDK in CI                                                               | never; drift gate local-only                                                                                                                                                                               | CI fetches the headers (from a secret or the Steamworks.NET mirror): drift checked on every push, licence posture of the mirror unclear         |
| Drift-gate input                                                        | the SDK headers as text, scanned by hand-written code; no new dependency                                                                                                                                   | `serde_json` over `steam_api.json`: a new direct edge, and the JSON lacks the lifecycle functions, `CallbackMsg_t` and sizes anyway             |
| `steam_appid.txt`                                                       | crate never writes it or sets env vars; error message says what is missing                                                                                                                                 | write it or `set_var`: convenient, but a side effect in the user's cwd or an `unsafe` env write with threads live                               |
| 32-bit and `aarch64-linux`                                              | 32-bit out of scope; `linuxarm64` path listed but unverified                                                                                                                                               | support 32-bit: a build and test matrix nothing else in the workspace has                                                                       |
| Microtransactions                                                       | declined (needs a server holding a publisher key)                                                                                                                                                          | build it: hosted infrastructure the project does not run                                                                                        |
| `Send` surfaces                                                         | `SteamTransport` and `SteamCloudStorage` are `Send` via a shared `Arc<Client>`, but call Steam only on the pump thread (checked; off-thread is a typed error); everything else `!Send`                     | all `!Send`: they could not implement `Transport`/`StorageSource`; or truly multi-threaded: rests on thread-safety Valve never states           |

## Review (step 2)

The first draft was reviewed on 2026-09-22 against the SDK 1.65 headers (every
function the catalogue names was checked to exist in `steam_api_flat.h`, every
accessor and version string, and every callback id cited) and against this tree
(every cited path and API). What changed, and why:

- **Wrong against the SDK, fixed.** `IsSteamRunningOnSteamDeck` no longer exists
  in `SteamUtils011`; slice 1 binds `IsRunningOnSteamHardware` instead.
  `GetVoice` has five deprecated arguments, not four. `CallbackMsg_t` lives in
  `steam_api_internal.h` and is 20 bytes on Linux/macOS versus 24 on Windows —
  now a slice 1 layout test, alongside Valve's packing sentinel.
  `GameOverlayActivated_t` is a friends callback, not a utils one. The current
  SDK is 1.65, not 1.63. The version strings are not uniformly `SteamXxxNNN`, so
  `versions.rs` stores both columns literally, and the handshake carries only
  strings Valve's own `InitEx` list carries (it omits timeline).
- **A claim that did not hold, replaced by a check.** The `Send` justification
  cited Valve thread-safety statements that do not exist as described. The
  `Send` surfaces now call Steam only on the pump thread, enforced at the
  boundary (R1).
- **Tests that could not fail, reworded.** The fake-`Lib` by-value-return test
  was presented as proving the ABI; a Rust callee cannot disagree with a Rust
  caller, so the ABI check is the real-controller run (R10). The planned
  "existing handshake/session tests generic over `T: Transport`" do not exist;
  slice 4 now extracts a conformance suite first and proves it against
  `InMemoryTransport`.
- **Testability gaps closed.** The single-owner guard moved into the `Lib` so
  parallel fake tests do not collide; the drift gate moved into the crate
  because it needs private tables; `pump` calls `ReleaseCurrentThreadMemory`,
  which `RunCallbacks` used to do for it; `EnableDeviceCallbacks` added for
  Steam Input device events; the overlay's launch-dependent injection on
  Linux/macOS is now a recorded manual step rather than a silent false negative.
- **No new dependency.** `serde_json` dropped: `steam_api.json` lacks the
  lifecycle functions, `CallbackMsg_t` and sizes, so the gate reads header text
  instead.
- **Slices resized.** Slice 1 is now loader, init, pump, `SteamId` and tests
  only; the umbrella feature, sandbox and remaining basics are 1b; the call
  registry moved to 3a with its first caller. Slice 3 split into 3a (lobbies,
  invites — EW's path) and 3b (persona, friends, avatars); slice 7 into 7a
  (seam), 7b (Steam Input), 7c (Deck text, glyphs).
- **EW's answers folded in.** The multi-session host is slice 2, built after 4,
  transport-generic with N a parameter, adding a transport-neutral "session
  ended" message so host-left is distinguishable on any transport; cloud via
  `ISteamRemoteStorage` and a minimal gamepad seam confirmed; the build order is
  EW's.

Verified in this tree: `Transport` and `TransportError` as described,
`SessionState::Reconnecting`, `reconnect_grace_period`, `Hello::session_token`,
`Server<T>` with one transport and `Server::reconnect`, `FileTransport`,
`StorageSource` with `Pending`/`Unsupported`, `NativeStorage::data`,
`MemoryStorage`, both test-local CRC-32s, `INTERNAL_SAMPLE_RATE`, `AudioSample`,
`Voice::new`, the `Device::Gamepad` "Nothing reports one yet" doc, `Binding`,
`virtual_stick`'s +Y up, `ShellEvent::TextCommit`, `assert_layout!`, the X11
`dlopen` declarations, the `crcbl-vk` loader rationale, `Loop::frame_body`,
`lose_focus`, `HostedGame`, the umbrella's `scene` feature, and the CI steps and
jobs named. **Not reviewed:** slices 9–15 beyond checking every function they
name exists; the Steam Runtime and macOS signing claims (still flagged
unverified); the `crcbl-store` synced-file protocol's classification rules,
which read as sound but were not tried against a model.

## Sources

Read 2026-08-22, and again 2026-09-22 where marked:

- [Steamworks API Overview](https://partner.steamgames.com/doc/sdk/api) —
  init/shutdown, `steam_appid.txt`, flat API, `steam_api.json`, linking.
- [steam_api.h reference](https://partner.steamgames.com/doc/api/steam_api) —
  `SteamAPI_Init`, `RestartAppIfNecessary`, `RunCallbacks`,
  `ReleaseCurrentThreadMemory`.
- [Steam Cloud](https://partner.steamgames.com/doc/features/cloud) (2026-09-22)
  — Auto-Cloud sync timing, Dynamic Cloud Sync, `GetLocalFileChange`, write
  batches.
- [SDK Access Agreement](https://partner.steamgames.com/documentation/sdk_access_agreement)
  — redistribution and source-use terms quoted above.
- SDK headers via the
  [Steamworks.NET mirror](https://github.com/rlabrecque/Steamworks.NET/tree/master/CodeGen/steam)
  (2026-09-22, at SDK 1.65: `steam_api_flat.h` for every accessor and signature
  quoted, `steam_api.h` for init and manual dispatch, `steam_api_internal.h` for
  `CallbackMsg_t` and the pipe getters, `steamclientpublic.h` for the packing
  selection and `ValvePackingSentinel_t`, `isteamutils.h` for
  `IsRunningOnSteamHardware`, `isteamuser.h` for voice, `isteamfriends.h` for
  rich-presence limits and `GameOverlayActivated_t`, `steamnetworkingtypes.h`
  and `isteamnetworkingsockets.h` for message limits, send flags, end-reason
  ranges and packing, `isteaminput.h` for the action-data packing and
  `RunFrame`, `steam_api.json` for callback ids and version strings). The
  mirror's commit history for `CodeGen/steam` dates the 1.64 and 1.65 updates.
- [ISteamNetworkingSockets](https://partner.steamgames.com/doc/api/ISteamNetworkingSockets)
  (2026-09-22) — read for a thread-safety statement; it has none.
- [SDK 1.63 release announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/627817201164877826),
  [SDK 1.61 announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/4480612432780198328),
  [SDK 1.62 patch notes](https://steamdb.info/patchnotes/17946746/).
- [Noxime/steamworks-rs](https://github.com/Noxime/steamworks-rs) — README,
  `steamworks-sys/build.rs` and `Cargo.toml`, for the rejected option.
