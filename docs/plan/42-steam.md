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

**Status (2026-09-22): planned, nothing built.** The four decisions the earlier
draft asked for were ratified 2026-09-06 (see "Decisions" below), and "the full
Steam API" is now in scope, which reverses two earlier "not now" calls — Steam
Input and `SteamTransport` — and pulls the first consumer's requirements (the
game EW, below) forward in the slice order.

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
  (`crcbl-steam/src/pump.rs`), because the crate does not exist yet and
  `tools/check-doc-citations.sh` checks only paths rooted at a top-level
  directory. The slice that creates a file switches its citations here to the
  rooted `crates/crcbl-steam/…` form in the same commit, so the gate starts
  checking them the moment they can resolve.
- **Paths inside the Steamworks SDK** (`public/steam/steam_api.json`,
  `redistributable_bin/…`) are external and never rooted at a repository
  directory, which is the opt-out: the gate does not ask them to exist.
- **Flat-API facts** (function names, accessor versions, struct packing) were
  re-read on 2026-09-22 from the header mirror Steamworks.NET maintains for its
  code generator
  ([rlabrecque/Steamworks.NET `CodeGen/steam`](https://github.com/rlabrecque/Steamworks.NET/tree/master/CodeGen/steam)),
  because the SDK zip is login-gated. Every one must be re-read from the SDK the
  implementer downloads; the drift gate (slice 1) is what makes that mechanical.
- **"Default (user to confirm)"** marks a decision this plan took on the user's
  behalf while they were away. Each is collected again under "Defaulted
  decisions" at the end, with the alternative and what changing it would cost.

## The first consumer: EW's requirements

EW (a crcbl game, co-op, player-hosted) sent its Steamworks requirements on
2026-09-22. They decide the slice order: every **hard** requirement is satisfied
by slice 7, and nothing EW does not use sits ahead of one it does.

| #   | EW requirement                                                                                                                                                                                                            | Weight              | Satisfied by                                                                                                                                                                 |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Listen-server co-op, squads of up to four friends, host authoritative; Steam networking (sockets, relay, P2P) as transport; friends/invites as the invitation path; reconnect after disconnect; clean host-leave handling | hard                | slice 3 (lobbies, invites, join paths) + slice 4 (`SteamTransport`, listener, end reasons, reconnect). The four-player _server_ is a `crcbl-server` gap — see "Engine seams" |
| 2   | No anti-cheat, no encrypted app tickets                                                                                                                                                                                   | hard (a "not")      | auth and encrypted tickets drop to slice 12; peer identity comes from the connection's Steam-certified identity instead (slice 4)                                            |
| 3   | Voice as raw decoded PCM into the game's own mixer; push-to-talk under game control; never Steam's playback                                                                                                               | hard                | slice 5                                                                                                                                                                      |
| 4   | Steam Input (Deck) arrives as the same `crcbl-input` device events as evdev/XInput, into backend-neutral snapshots, not a separate path                                                                                   | hard                | slice 7, on the gamepad seam defined in "Engine seams"                                                                                                                       |
| 5   | Cloud: whole-file sync of one atomically written profile file; conflicts surfaced to the game; never merged or partially applied                                                                                          | hard                | slice 6 (`SteamCloudStorage` + the synced-file protocol in `crcbl-store`). Auto-Cloud cannot meet it — see "Cloud"                                                           |
| 6   | Local Steam ID as a stable profile and participant identity                                                                                                                                                               | hard                | slice 1 (`Steam::user().steam_id()`), and slice 4 for the remote side                                                                                                        |
| —   | Rich presence, achievements and stats, overlay and friends list for invites                                                                                                                                               | wanted              | slice 3 (presence, overlay, friends), slice 9 (achievements, stats)                                                                                                          |
| —   | Workshop, inventory, leaderboards, timeline, DLC                                                                                                                                                                          | unused by EW, scope | slices 9–15                                                                                                                                                                  |

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
  per ABI revision. As read from the mirror on 2026-09-22 (the list the version
  handshake is built from):

  | Interface                  | Accessor                                         | Game-server twin                                           |
  | -------------------------- | ------------------------------------------------ | ---------------------------------------------------------- |
  | `ISteamUser`               | `SteamAPI_SteamUser_v023`                        | —                                                          |
  | `ISteamFriends`            | `SteamAPI_SteamFriends_v018`                     | —                                                          |
  | `ISteamUtils`              | `SteamAPI_SteamUtils_v011`                       | `SteamAPI_SteamGameServerUtils_v011`                       |
  | `ISteamMatchmaking`        | `SteamAPI_SteamMatchmaking_v009`                 | —                                                          |
  | `ISteamMatchmakingServers` | `SteamAPI_SteamMatchmakingServers_v003`          | —                                                          |
  | `ISteamParties`            | `SteamAPI_SteamParties_v002`                     | —                                                          |
  | `ISteamRemoteStorage`      | `SteamAPI_SteamRemoteStorage_v016`               | —                                                          |
  | `ISteamUserStats`          | `SteamAPI_SteamUserStats_v013`                   | `SteamAPI_SteamGameServerStats_v001`                       |
  | `ISteamApps`               | `SteamAPI_SteamApps_v009`                        | —                                                          |
  | `ISteamScreenshots`        | `SteamAPI_SteamScreenshots_v003`                 | —                                                          |
  | `ISteamInput`              | `SteamAPI_SteamInput_v007`                       | —                                                          |
  | `ISteamUGC`                | `SteamAPI_SteamUGC_v021`                         | `SteamAPI_SteamGameServerUGC_v021`                         |
  | `ISteamInventory`          | `SteamAPI_SteamInventory_v003`                   | `SteamAPI_SteamGameServerInventory_v003`                   |
  | `ISteamTimeline`           | `SteamAPI_SteamTimeline_v004`                    | —                                                          |
  | `ISteamRemotePlay`         | `SteamAPI_SteamRemotePlay_v004`                  | —                                                          |
  | `ISteamNetworkingSockets`  | `SteamAPI_SteamNetworkingSockets_SteamAPI_v013`  | `SteamAPI_SteamGameServerNetworkingSockets_SteamAPI_v013`  |
  | `ISteamNetworkingMessages` | `SteamAPI_SteamNetworkingMessages_SteamAPI_v002` | `SteamAPI_SteamGameServerNetworkingMessages_SteamAPI_v002` |
  | `ISteamNetworkingUtils`    | `SteamAPI_SteamNetworkingUtils_SteamAPI_v004`    | (shared)                                                   |
  | `ISteamGameServer`         | `SteamAPI_SteamGameServer_v015`                  | —                                                          |

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
  of the accessors it binds** (`"SteamUser023\0SteamFriends018\0…\0\0"`, derived
  from one table in `crcbl-steam/src/ffi/versions.rs`, never typed twice), so a
  client that cannot honour them fails init with
  `k_ESteamAPIInitResult_VersionMismatch` and an English `SteamErrMsg`
  (`typedef char SteamErrMsg[1024]`). `ESteamAPIInitResult` is
  `OK = 0, FailedGeneric = 1, NoSteamClient = 2, VersionMismatch = 3`.
  `SteamAPI_InitFlat` is not bound.

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
  Asynchronous results arrive as `SteamAPICallCompleted_t` (id
  `k_iSteamUtilsCallbacks + 3`, `k_iSteamUtilsCallbacks = 700`) carrying
  `{ SteamAPICall_t m_hAsyncCall; int m_iCallback; uint32 m_cubParam; }`, and
  the payload is fetched with `GetAPICallResult`. `SteamAPICall_t` is `uint64`
  with `0` reserved as `k_uAPICallInvalid`; `HSteamPipe` and `HSteamUser` are
  `int32`. The pipe comes from `SteamAPI_GetHSteamPipe()` (the game-server pipe
  from `SteamGameServer_GetHSteamPipe()`). **This is the ABI `crcbl-steam` binds
  — bytes and integers on a pipe we poll, no C++ vtables registered with anyone,
  and no foreign code calling back into Rust.**

- **The SDK ships `steam_api.json`**, a machine-readable description of every
  interface, method, callback struct and constant. It is the input the drift
  gate reads. The Steamworks.NET mirror carries a copy; this repository does
  not.
- **Version facts.** Current SDK is 1.63 (2026-01-29:
  [announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/627817201164877826)
  — adds linuxarm64/androidarm64 libs, removes `ISteamMusicRemote`); 1.61
  removed `RequestCurrentStats`
  ([1.61 announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/4480612432780198328));
  1.62 removed `ISteamFriends::SetPersonaName` and `GetUserRestrictions`
  ([1.62 notes](https://steamdb.info/patchnotes/17946746/)). `steamworks-rs`
  pins 1.64, for which no announcement was found; read the number off the zip.

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
  read by (1) the drift gate, from `public/steam/steam_api.json`, and (2) the
  runtime library search, from `redistributable_bin/<platform>/`, as the
  development fallback after "next to the executable".
- **CI never has the SDK.** Default (user to confirm): no CI job fetches it —
  not from a secret, not from a private mirror, and not from Steamworks.NET's
  public copy of `steam_api.json`, whose own licence posture this project should
  not lean on. The drift gate is therefore a local gate, run by the developer
  who touches the declarations (see "What CI proves"). The gate reads any
  `steam_api.json` path, so flipping this later is a CI-step change, not a code
  change.
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
  check.

**Why the loaded module is never unloaded:** interface pointers and the callback
buffers Steam hands out point into it, so unloading while anything might hold
one is a use-after-free. `crcbl-shell` leaks its `dlopen` handles for the same
reason. The `Lib` (module + resolved symbols) is cached in a `OnceLock`;
interface pointers are not (see "Ownership").

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
implement their traits. Default (user to confirm): `serde_json` as a
**dev-dependency** for the drift gate — it is already in `Cargo.lock`
transitively (through `gltf`), so no new crate enters the graph, but it is a new
direct edge and that is the user's call; the fallback is a small `.mjs` script
under `tools/` doing the same comparison.

Target gating follows the `crcbl-dx12` pattern — no `#![cfg(...)]` crate root:

- Every module that touches the FFI is
  `#[cfg(all(target_pointer_width = "64", any(target_os = "linux", target_os = "windows", target_os = "macos")))]`.
  Elsewhere the crate is its documentation and no public items; nothing above
  the crate ever writes `cfg(target_os)` to ask about Steam.
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
- The umbrella exposes it as `crates/crcbl/Cargo.toml`'s existing optional-dep
  shape: `steam = ["dep:crcbl-steam"]`, beside `scene = ["dep:crcbl-scene", …]`,
  and `pub use crcbl_steam as steam;` behind it.

## The binding route — decided

**Hand-written flat-API declarations + runtime loading** (ratified 2026-09-06).
The `crcbl-shell` pattern applied verbatim, laid out as:

- `crcbl-steam/src/ffi/mod.rs` — scalar aliases (`HSteamPipe = i32`,
  `SteamAPICall = u64`, `AppId = u32`, …), and the `Lib` struct of typed
  function pointers, grouped per interface (`Lib.user`, `Lib.friends`, …) so a
  slice adds one group.
- `crcbl-steam/src/ffi/prototype.rs` — one `type` per function pointer, each
  carrying in its doc comment **the C declaration it was copied from and the SDK
  version it was read from**.
- `crcbl-steam/src/ffi/structs.rs` — the `repr(C)` structs (callback payloads,
  `SteamNetworkingIdentity`, `InputAnalogActionData_t`, …), packing selected per
  OS, with the field-by-field layout table (below).
- `crcbl-steam/src/ffi/versions.rs` — the accessor/interface-version table
  (`("SteamAPI_SteamUser_v023", "SteamUser023")`, …), the single source for both
  accessor lookup and the init handshake string.
- `crcbl-steam/src/ffi/load.rs` — the per-OS loader and the `symbol!` macro that
  resolves a name or returns `InitError::NoSymbol(name)`.
- `crcbl-steam/src/ffi/manifest.rs` — a `const BINDINGS: &[BoundFn]` table
  (`{ name, returns: "uint64_steamid", params: &["ISteamUser*"] }`) written from
  the same header read, which is what the drift gate compares against
  `steam_api.json`. The `symbol!` loads are generated from the same list by a
  declarative macro, so a function cannot be loaded without being in the
  manifest.

**Only what a slice uses is declared** — the flat header has on the order of a
thousand functions; each slice binds its dozens. Two checks keep the
declarations honest: the version handshake at init, and the drift gate.

**The layout tests** follow `crates/crcbl-shell/src/win32/ffi.rs`'s
`assert_layout!` exactly — size, then every field's offset and width, with a
destructuring pattern that makes a field without a row a compile error. Each
struct's table is per-OS where the packing differs (see the traps section), and
the numbers come from the SDK's own `sizeof`/`offsetof`, printed by a C program
compiled against the downloaded headers on each OS — the program is described in
`crcbl-steam/src/ffi/structs.rs`'s docs, run locally, and its output pasted as
the table, the same provenance the Win32 table documents. Because the
`test-cross-platform` matrix runs natively, the Windows (`pack(8)`) and
Linux/macOS (`pack(4)`) tables both execute in CI.

**The drift gate** (`crcbl-steam/tests/drift.rs`, `#[ignore]`d): with
`CRCBL_STEAM_SDK` set, parse `public/steam/steam_api.json` and assert that every
`BINDINGS` entry exists with the same return type and parameter types, that
every `versions.rs` accessor exists, and that every bound callback struct's
field list and `k_iCallback` id match. Without the variable it **fails** (it is
only ever run on purpose, and "skipped" must not read as "passed"). Proven red
before trusted: rename one parameter type in the manifest, watch it fail, and
restore.

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
    /// library only transiently; `Ok(true)` means "quit now, Steam is
    /// relaunching us". Always `Ok(false)` while `steam_appid.txt` exists.
    pub fn relaunch_via_steam(app: AppId) -> Result<bool, InitError>;

    /// Load the library, `SteamInternal_SteamAPI_Init` with the bound version
    /// list, `SteamAPI_ManualDispatch_Init`, resolve and null-check every
    /// bound accessor. At most one live `Steam` per process: a second call
    /// while one lives is `Err(InitError::AlreadyInitialised)`.
    pub fn init(app: AppId) -> Result<Steam, InitError>;
}
```

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
- **Which surfaces are `Send`, and why that is sound.** `Client` is
  `Send + Sync` by an `unsafe impl` whose justification names Valve's own
  statements: `ISteamNetworkingSockets` documents that its functions may be
  called from any thread (its header: "you may call Release() from any thread",
  and the connection-status and message paths are internally locked), and the
  Steam API as a whole is documented as callable from threads other than the
  main one provided `SteamAPI_ReleaseCurrentThreadMemory` is called on them. The
  `Send` surfaces are therefore restricted to the interfaces whose methods they
  call — networking sockets and utils for `SteamTransport`, remote storage for
  `SteamCloudStorage` — and each `Send` type calls
  `SteamAPI_ReleaseCurrentThreadMemory` after use from a thread that is not the
  pump's (it tracks the pump thread's `ThreadId`). Everything else is reachable
  only through `!Send` `Steam`. **Needs review:** the thread-safety of
  `ISteamRemoteStorage`'s synchronous file calls is inferred from the general
  statement, not stated for that interface — see "Risks".
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
   is still alive on another thread, shutdown waits for it by construction.
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
    NoSteamClient { message: String, cwd: PathBuf, appid_file: bool },
    VersionMismatch(String),                           // Valve's SteamErrMsg
    Failed(String),                                    // FailedGeneric
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
    /// `ISteamInput::RunFrame` when input is initialised, then the
    /// GetNextCallback / FreeLastCallback loop. Payloads decode to
    /// `SteamEvent`s; `SteamAPICallCompleted_t` routes to the call registry;
    /// connection-status changes also update the shared networking state.
    /// Once per frame.
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
- **Async calls are typed tokens redeemed at the pump:**

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
- **Payload decode is `repr(C)` structs with the platform's packing,
  size-checked before read** — `m_cubParam` compared to `size_of`, then one
  `read_unaligned` copy-out, fields only ever copied.
- **`k_iCallback` ids live in one table** (`crcbl-steam/src/callbacks.rs`), each
  row `(id, name, size-per-OS, decode fn)`, each id written as Valve's base plus
  offset (`K_I_STEAM_FRIENDS_CALLBACKS + 31`), and each checked by the drift
  gate against `steam_api.json`.

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
  Steam-owned message escapes.
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
- **What is not Steam's job:** `crcbl-server`'s `Server<T: Transport>`
  (`crates/crcbl-server/src/lib.rs`) owns **one** transport and one
  `SessionManager`. A host serving three remote peers plus its own local client
  needs a multi-session server — N transports, N sessions, one world. That is
  `crcbl-server`/`crcbl-net` work outside this topic, required by EW requirement
  1 regardless of transport, and recorded in the backlog. Until it lands, slice
  4's end-to-end test is one host and one peer, and EW's own host code may fan
  out over several `SteamTransport`s itself.

`ISteamNetworkingMessages` (connectionless, UDP-shaped) is catalogued but not
used by `Transport`; it lands on demand (slice 14 catalogue) if a game wants
unconnected pings. The deprecated `ISteamNetworking` is never bound.

### Invitations — lobbies, rich presence, overlay

No engine seam exists for "a group of friends about to play", and none is
invented: slice 3's `Lobby` is a Steam type the game's menu drives, and the seam
it feeds is the one above — a lobby's owner `SteamId` is what a joiner connects
`SteamTransport` to. The flow EW needs:

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
5. Host leaves: the joiner sees both the transport end reason `HostLeft` and
   `SteamEvent::LobbyMemberChanged { member: owner, change: Left }`; Steam
   passes lobby ownership on automatically, and EW treats the session as over.

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

So slice 7 depends on a **gamepad seam**, specified here as the minimum both
Steam Input and evdev need, and landed by whichever of the two arrives first
(default (user to confirm): if topic 19's evdev slice has not landed when slice
7 starts, slice 7 lands the seam in `crcbl-input` as its own reviewable commit,
before any Steam code):

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

pub enum Binding { /* … */ PadButton(PadButton), PadStick { stick: Stick, deadzone: f32 },
                   PadTrigger { trigger: Trigger, threshold: f32 } }
```

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
configuration — so it works under app 480.

**Double input must be impossible.** With Steam Input active for the app, Steam
also exposes a virtual XInput/evdev pad (Valve's USB vendor `0x28DE`). The rule
the seam carries: one backend owns a physical pad. When the Steam Input backend
is live, the evdev/XInput backends skip Valve's virtual devices, and the Steam
Input backend reports the pad; when Steam is absent, the native backends see the
real device. Slice 7 adds that filter to whichever native backend exists, with a
test.

Also in slice 7, Deck-shaped `ISteamUtils` calls: `IsSteamRunningOnSteamDeck`
(slice 1 already reports it), `ShowGamepadTextInput` /
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
`OverlayActivated { active: true }` through it, and until then each app calls it
itself. **Whether the overlay composites over `crcbl-shell`'s own
Wayland/X11/Win32/AppKit windows and each GPU backend's swapchain is
unverified**, and stays a named line in every slice's manual checklist.

## Interface catalogue

Every client-side Steamworks interface in the current SDK, with the slice that
lands it or the reason it does not. "EW" marks what the first consumer needs.

| Interface / area                                                                          | What is bound                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Slice                                                                                                                                                                                                                                                                                                                                                     | EW                   |
| ----------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| Lifecycle                                                                                 | `SteamInternal_SteamAPI_Init`, `SteamAPI_RestartAppIfNecessary`, `SteamAPI_Shutdown`, `SteamAPI_GetHSteamPipe`, `SteamAPI_ReleaseCurrentThreadMemory`, manual dispatch (all five), `SteamAPI_IsSteamRunning`                                                                                                                                                                                                                                                                    | 1                                                                                                                                                                                                                                                                                                                                                         | yes                  |
| Call results                                                                              | `SteamAPICallCompleted_t` + `GetAPICallResult`, `ISteamUtils::IsAPICallCompleted`/`GetAPICallFailureReason`                                                                                                                                                                                                                                                                                                                                                                     | 1                                                                                                                                                                                                                                                                                                                                                         | yes                  |
| `SteamAPI_RunCallbacks`, `SteamAPI_InitFlat`                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | never — excluded by manual dispatch / no version check                                                                                                                                                                                                                                                                                                    | —                    |
| `ISteamUser` identity                                                                     | `GetSteamID`, `BLoggedOn`, `GetPlayerSteamLevel`                                                                                                                                                                                                                                                                                                                                                                                                                                | 1                                                                                                                                                                                                                                                                                                                                                         | yes (6)              |
| `ISteamUtils` basics                                                                      | `GetAppID`, `GetSteamUILanguage`, `IsOverlayEnabled`, `IsSteamRunningOnSteamDeck`, `IsSteamInBigPictureMode`, `SetOverlayNotificationPosition`/`Inset`, `GetServerRealTime`, `GetIPCountry`, `GameOverlayActivated_t`                                                                                                                                                                                                                                                           | 1                                                                                                                                                                                                                                                                                                                                                         | yes                  |
| `ISteamApps` basics                                                                       | `BIsSubscribed`, `GetCurrentGameLanguage`, `GetLaunchCommandLine`, `NewUrlLaunchParameters_t`                                                                                                                                                                                                                                                                                                                                                                                   | 1 (basics), 3 (launch line)                                                                                                                                                                                                                                                                                                                               | yes                  |
| `ISteamFriends`                                                                           | persona (own, friends', `PersonaStateChange_t`), friends list, avatars (+ `ISteamUtils::GetImageSize`/`GetImageRGBA`, `AvatarImageLoaded_t`), rich presence set/clear/read, overlay dialogs (`ActivateGameOverlay`, `…ToUser`, `…ToWebPage`, `…InviteDialog`), `InviteUserToGame`, join-request callbacks                                                                                                                                                                       | 3                                                                                                                                                                                                                                                                                                                                                         | wanted + invites (1) |
| `ISteamMatchmaking` (lobbies)                                                             | create/join/leave, members, owner, data, member data, chat-update/data-update/enter callbacks, lobby chat messages                                                                                                                                                                                                                                                                                                                                                              | 3                                                                                                                                                                                                                                                                                                                                                         | yes (1)              |
| `ISteamNetworkingSockets`                                                                 | P2P listen/connect/accept/close, send/receive, connection info and real-time status, poll groups, status-changed callback                                                                                                                                                                                                                                                                                                                                                       | 4                                                                                                                                                                                                                                                                                                                                                         | yes (1)              |
| `ISteamNetworkingUtils`                                                                   | `InitRelayNetworkAccess`, `GetRelayNetworkStatus`, `SteamRelayNetworkStatus_t`, ping location (for "region" display)                                                                                                                                                                                                                                                                                                                                                            | 4                                                                                                                                                                                                                                                                                                                                                         | yes (1)              |
| `ISteamUser` voice                                                                        | `StartVoiceRecording`, `StopVoiceRecording`, `GetAvailableVoice`, `GetVoice`, `DecompressVoice`, `GetVoiceOptimalSampleRate`                                                                                                                                                                                                                                                                                                                                                    | 5                                                                                                                                                                                                                                                                                                                                                         | yes (3)              |
| `ISteamRemoteStorage`                                                                     | `FileWrite`, `FileRead`, `FileExists`, `FileDelete`, `GetFileSize`, `GetFileTimestamp`, `GetFileCount`/`GetFileNameAndSize`, quota, `IsCloudEnabledForAccount`/`ForApp`, `SetCloudEnabledForApp`, write batches, local-file-change                                                                                                                                                                                                                                              | 6                                                                                                                                                                                                                                                                                                                                                         | yes (5)              |
| `ISteamInput`                                                                             | `Init(true)`, `RunFrame`, `Shutdown`, `SetInputActionManifestFilePath`, controllers, action sets, digital/analog action data, origins and glyphs, `GetInputTypeForHandle`, vibration/LED, `ShowBindingPanel`, connected/disconnected callbacks                                                                                                                                                                                                                                  | 7                                                                                                                                                                                                                                                                                                                                                         | yes (4)              |
| `ISteamUtils` Deck text                                                                   | `ShowGamepadTextInput`, `GetEnteredGamepadTextInput`, `ShowFloatingGamepadTextInput`, `DismissFloatingGamepadTextInput`, their dismissed callbacks                                                                                                                                                                                                                                                                                                                              | 7                                                                                                                                                                                                                                                                                                                                                         | yes (4)              |
| `ISteamUserStats` achievements + stats                                                    | set/clear/get achievement, achievement display attributes and icon, int/float stats, `StoreStats`, `IndicateAchievementProgress`, global achievement percentages, `UserStatsReceived_t`/`Stored_t`/`UserAchievementStored_t`                                                                                                                                                                                                                                                    | 9                                                                                                                                                                                                                                                                                                                                                         | wanted               |
| `ISteamUserStats` leaderboards                                                            | find/find-or-create, upload score (with details), download entries (global, around user, friends, users), attach UGC                                                                                                                                                                                                                                                                                                                                                            | 9                                                                                                                                                                                                                                                                                                                                                         | no                   |
| `ISteamScreenshots`                                                                       | `TriggerScreenshot`, `HookScreenshots` + `ScreenshotRequested_t`, `WriteScreenshot`, tag user/location                                                                                                                                                                                                                                                                                                                                                                          | 10                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamTimeline` (game recording)                                                         | tooltip, game mode, instantaneous/range events, game phases and their tags/attributes, "does recording exist" calls, open overlay to event/phase                                                                                                                                                                                                                                                                                                                                | 10                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamApps` rest                                                                         | DLC (`BIsDlcInstalled`, `GetDLCCount`, `BGetDLCDataByIndex`, `InstallDLC`/`UninstallDLC`, `DlcInstalled_t`), betas (`GetCurrentBetaName`, `GetNumBetas`/`GetBetaInfo`/`SetActiveBeta`, all in the 2026-09-22 mirror), ownership (`BIsSubscribedApp`, `BIsLowViolence`, `BIsVACBanned`, `GetEarliestPurchaseUnixTime`, `BIsSubscribedFromFreeWeekend`, `BIsSubscribedFromFamilySharing`, `GetAppOwner`), `GetAppInstallDir`, `GetAppBuildId`, `MarkContentCorrupt`, file details | 11                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamRemotePlay`                                                                        | session count/info, `BSendRemotePlayTogetherInvite`, session connected/disconnected callbacks                                                                                                                                                                                                                                                                                                                                                                                   | 11                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamUser` auth                                                                         | `GetAuthSessionTicket` (with `SteamNetworkingIdentity`), `GetAuthTicketForWebApi` + `GetTicketForWebApiResponse_t`, `BeginAuthSession`/`EndAuthSession` (peer-to-peer validation), `CancelAuthTicket`, `UserHasLicenseForApp`, `ValidateAuthTicketResponse_t`                                                                                                                                                                                                                   | 12                                                                                                                                                                                                                                                                                                                                                        | no (2)               |
| Encrypted app tickets                                                                     | `RequestEncryptedAppTicket` → `EncryptedAppTicketResponse_t`, `GetEncryptedAppTicket`; server-side decryption via the separate `sdkencryptedappticket` library, loaded the same way                                                                                                                                                                                                                                                                                             | 12                                                                                                                                                                                                                                                                                                                                                        | no (2)               |
| Game server                                                                               | `SteamInternal_GameServer_Init_V2`, `SteamGameServer_Shutdown`/`GetHSteamPipe`/`BSecure`/`GetSteamID`, `ISteamGameServer` (logon anonymous/token, server info, auth sessions, `UserHasLicenseForApp`, advertise), `ISteamGameServerStats`, game-server networking sockets, `ISteamMatchmakingServers` (server browser)                                                                                                                                                          | 13                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamUGC` (Workshop)                                                                    | query (all/user/details), subscribe/unsubscribe, item state and install info, download, create + update + submit, `ItemInstalled_t`, `DownloadItemResult_t`                                                                                                                                                                                                                                                                                                                     | 14                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamInventory`                                                                         | result handles, `GetAllItems`, `GetResultItems`, item definitions and properties, grant promo, consume, exchange, `StartPurchase`, prices                                                                                                                                                                                                                                                                                                                                       | 15                                                                                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamParties`                                                                           | beacons (advertise an open slot in the friends list)                                                                                                                                                                                                                                                                                                                                                                                                                            | on demand — lobbies + rich presence already cover EW's invite path                                                                                                                                                                                                                                                                                        | no                   |
| `ISteamNetworkingMessages`                                                                | connectionless send/receive                                                                                                                                                                                                                                                                                                                                                                                                                                                     | on demand                                                                                                                                                                                                                                                                                                                                                 | no                   |
| `ISteamParentalSettings`                                                                  | parental lock queries                                                                                                                                                                                                                                                                                                                                                                                                                                                           | on demand (a store requirement if the game has content restrictions)                                                                                                                                                                                                                                                                                      | no                   |
| `ISteamVideo`, `ISteamMusic`                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: broadcast/music-player control, no engine use                                                                                                                                                                                                                                                                                                   | no                   |
| `ISteamHTMLSurface`                                                                       | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: an embedded browser renderer, no engine UI can host it                                                                                                                                                                                                                                                                                          | no                   |
| `ISteamHTTP`                                                                              | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: no engine feature needs an HTTP client, and one that did would not tie it to Steam                                                                                                                                                                                                                                                              | no                   |
| `ISteamNetworking` (old P2P), `ISteamController`                                          | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | declined: deprecated by Valve in favour of `ISteamNetworkingSockets` and `ISteamInput`                                                                                                                                                                                                                                                                    | no                   |
| `ISteamMicroTransactions`                                                                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | **declined** — there is no such client interface: microtransactions are the `ISteamMicroTxn` **Web API**, called from a server holding the publisher Web API key; the client only receives `MicroTxnAuthorizationResponse_t`. That is hosted infrastructure the project does not run. Reopen only with a backend; the one callback is trivial to add then | no                   |
| `ISteamAppList`, `ISteamMusicRemote`, `ISteamGameCoordinator`, `ISteamPS3OverlayRenderer` | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | removed from the SDK or not for PC games                                                                                                                                                                                                                                                                                                                  | no                   |

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
accounts on two machines (one may be a Deck).

### Slice 1 — Loader, lifecycle, pump, local identity

- **Scope:** crate skeleton and CI wiring; the loader on all three OSes;
  `Steam::init` with the version handshake; `relaunch_via_steam`; `Client` +
  `Arc` shutdown; manual-dispatch pump; call registry;
  `SteamEvent::OverlayActivated`; `ISteamUser` identity, `ISteamUtils` basics
  (including `IsSteamRunningOnSteamDeck`), `ISteamApps` basics;
  `PumpDiagnostics`; the fake-`Lib` rig; the drift gate; `.gitignore` lines;
  umbrella `steam` feature; `apps/sandbox` behind it calling init/pump and
  logging identity and overlay events; `ROADMAP.md` phase claim.
- **Files:** `crcbl-steam/Cargo.toml`, `crcbl-steam/src/lib.rs`,
  `crcbl-steam/src/ffi/{mod,prototype,structs,versions,load,manifest}.rs`,
  `crcbl-steam/src/{client,pump,callbacks,call,error,user,utils,apps}.rs`,
  `crcbl-steam/src/testing.rs` (`#[cfg(test)]` fake `Lib`),
  `crcbl-steam/tests/{drift,smoke}.rs`; `Cargo.toml` (workspace dep),
  `crates/crcbl/Cargo.toml`, `.github/workflows/ci.yml`, `.gitignore`,
  `apps/sandbox/Cargo.toml` and its main.
- **API:**

  ```rust
  let mut steam = Steam::init(AppId(480))?;
  let me: SteamId = steam.user().steam_id();      // EW 6: stable u64
  let name: String = steam.friends().persona_name();
  let deck: bool = steam.utils().is_steam_deck();
  steam.pump();
  for event in steam.events() { if let SteamEvent::OverlayActivated { active } = event { /* pause */ } }
  ```

- **Tests that can fail (CI):** layout tables for `CallbackMsg_t`,
  `SteamAPICallCompleted_t`, `GameOverlayActivated_t` per OS; fake-`Lib` drain
  tests — a scripted sequence with an unknown id (skipped, counted), a claimed
  id with the wrong size (counted in `decode_mismatches`, not decoded), a
  completion nobody registered (counted), a completion that decodes, and
  **`FreeLastCallback` called exactly once per `GetNextCallback` true** (the
  fake counts; a drain loop that leaks or double-frees fails); init-result
  mapping for each `ESteamAPIInitResult`; the handshake string equals the
  `versions.rs` table joined with NULs and double-terminated; an accessor
  returning null is `NoInterface`; a second `init` while one lives is
  `AlreadyInitialised`; `NoLibrary` lists every path tried in order; interior
  NUL → `InteriorNul`. Each test broken once by hand (e.g. skip the free) and
  seen red before it is trusted.
- **CI verifies:** all of the above on Linux, Windows and macOS natively; the
  cross-target clippy and doc steps; Miri over decode and drain; the wasm job
  (empty crate).
- **Needs a real client:** `smoke.rs` (`#[ignore]`): init succeeds under 480,
  `steam_id()` is non-zero and stable across two inits in two processes, pump
  runs for 300 frames with `decode_mismatches == 0`, `OverlayActivated` arrives
  when Shift+Tab is pressed (manual step), shutdown then process exit is clean.
  The drift gate passes against the downloaded SDK and fails after a deliberate
  manifest edit. Manual: `apps/sandbox --features steam` on each OS shows the
  overlay over the window and pauses; launch without Steam running →
  `NoSteamClient` logged, sandbox runs on; launch with no redistributable →
  `NoLibrary` with the paths; launch without `steam_appid.txt` → the message
  says so.
- **Exit:** CI green on all jobs; smoke + drift gate run and pass on all three
  OSes (or the OS not run is named as a gap in the backlog); overlay composition
  result recorded per OS and GPU backend.

### Slice 2 — (reserved: engine seams owed by EW before Steam can use them)

Not Steam code, listed so the order is honest: **the multi-session server**
(`crcbl-server` hosting N `Transport`s) and **the gamepad seam** in
`crcbl-input` (sketched above). Neither is inside this topic; both are EW
requirements regardless of Steam. Slice 4's full four-player test waits on the
first, slice 7 on the second (or lands it, per its default). This slice number
exists so later slices can say "after 2" rather than hide the dependency.

### Slice 3 — Friends, presence, overlay dialogs, lobbies, invites

- **Scope:** `ISteamFriends` persona/friends/avatars/rich presence/overlay
  dialogs/invites; `ISteamMatchmaking` lobbies; the join paths (both callbacks,
  launch command line, `NewUrlLaunchParameters_t`). EW requirement 1's
  invitation path and its "wanted" rich presence, overlay and friends list.
- **Files:** `crcbl-steam/src/{friends,matchmaking,avatar}.rs`, additions to
  `ffi/`, `callbacks.rs`; `apps/sandbox` lobby panel behind the feature.
- **API:**

  ```rust
  let call = steam.matchmaking().create_lobby(LobbyKind::FriendsOnly, 4)?;
  // later frame:
  if let CallState::Ready(created) = steam.take(call) { let lobby: Lobby = created.lobby()?; }
  steam.friends().open_invite_dialog(lobby.id());
  steam.friends().set_rich_presence("connect", &format!("+connect_lobby {}", lobby.id().0))?;
  match event { SteamEvent::LobbyJoinRequested { lobby, .. } => steam.matchmaking().join_lobby(lobby), … }
  let owner: SteamId = lobby.owner(&steam);
  let avatar: Option<Rgba> = steam.friends().small_avatar(friend); // None until AvatarImageLoaded
  ```

  `Lobby` is RAII (`Drop` → `LeaveLobby`), `!Send`, holds no Steam borrow (it
  takes `&Steam` per call) so a game can store it.

- **Tests (CI):** layout tables for every new callback per OS; decode of each
  from a byte fixture; `+connect_lobby` parsing from launch command lines
  (present, absent, malformed, trailing args) — pure; the lobby-member tracking
  state machine (enter, member joined/left/disconnected/kicked, owner change)
  from scripted callbacks; rich-presence key/value limits
  (`k_cchMaxRichPresenceKeyLength` and value length) rejected before the call;
  avatar RGBA size check.
- **Needs a real client (two accounts):** A creates a friends-only lobby, B sees
  A as "in game" with rich presence; A invites via overlay; B accepts **with the
  game running** (join event) and **with it closed** (Steam launches with
  `+connect_lobby`); B's member list shows A as owner; A quits → B gets
  member-left for the owner. On each OS at least once as A and once as B.
- **Exit:** all four join paths demonstrated on at least two OSes and the rest
  named; decode mismatches zero throughout.

### Slice 4 — `SteamTransport` and `SteamListener` (P2P + relay)

- **Scope:** everything under "Networking" above. After slice 3 (admission reads
  lobby membership). Implements `crcbl_net::Transport`.
- **Files:**
  `crcbl-steam/src/net/{mod,transport,listener,identity,end_reason}.rs`;
  `crcbl-steam/Cargo.toml` gains `crcbl-net`; a `crcbl-steam/tests/net_smoke.rs`
  (`#[ignore]`); `apps/sandbox` connects the lobby owner and exchanges a
  `crcbl-net` handshake.
- **API:**

  ```rust
  let listener = SteamListener::open(&steam, &lobby, VirtualPort(0))?;   // host
  while let Some(peer) = listener.accept(&mut steam) { let who: SteamId = peer.remote(); server.add(peer); }
  let link = SteamTransport::connect(&steam, lobby.owner(&steam), VirtualPort(0))?; // joiner
  // both: impl crcbl_net::Transport for SteamTransport (Send)
  link.end_reason() // Option<EndReason>: HostLeft | Kicked | ServerFull | ShuttingDown | Lost(code)
  ```

- **Tests (CI):** layout tables for `SteamNetworkingIdentity` and
  `SteamNetConnectionStatusChangedCallback_t` (the former is `pack(1)` in
  `steamnetworkingtypes.h`, the latter platform-packed — tables must differ
  where the header says they do); the transport over a fake `Lib` whose
  send/receive are an in-process loop: reliable before unreliable on `recv`,
  `MessageTooLarge` at the limit + 1 and not at the limit, `Backpressure` on
  `k_EResultLimitExceeded`, every received message released exactly once (the
  fake counts), `is_connected` follows the fake's connection state; end-reason
  mapping for each SDK end code and each app code; listener admission: a
  non-member's `Connecting` is closed, a member's accepted — the admission test
  broken once (accept everyone) and seen red. **`crcbl-net`'s own conformance**:
  run the existing handshake/session tests generic over `T: Transport` against
  `SteamTransport` on the fake loop, so the Steam transport is held to the same
  behaviour as `InMemoryTransport`.
- **Needs a real client (two accounts, two machines, ideally two networks):**
  host + joiner connect through the lobby; handshake completes; 10 minutes of
  snapshots at the sample's tick rate with no disconnect; relay status reaches
  "current" before connect; pull the joiner's network for longer than a few
  seconds but under `reconnect_grace_period` → joiner reconnects and resumes
  with its `ResumeToken`; host quits → joiner sees `HostLeft`, not `Lost`; third
  account not in the lobby attempting `ConnectP2P` to the host is refused. Once
  across NAT (two homes, or a phone hotspot) to prove the relay path.
- **Exit:** conformance tests green; two-machine run on Windows↔Linux and
  Linux↔macOS at least; four-player run recorded once the multi-session server
  (slice 2) exists, and named as a gap until then.

### Slice 5 — Voice to PCM

- **Scope:** `ISteamUser` voice; push-to-talk under game control; PCM out. EW
  requirement 3.
- **Files:** `crcbl-steam/src/voice.rs`.
- **API:**

  ```rust
  let mut mic = steam.voice().capture()?;       // VoiceCapture: StartVoiceRecording; Drop stops
  mic.set_transmitting(ptt_held);               // game's push-to-talk: start/stop recording
  while let Some(packet) = mic.poll(&steam)? { link.send_unreliable(Message::unreliable(packet.into_bytes()))?; }
  // receiver, per speaker:
  let pcm: Vec<crcbl_audio::AudioSample> = steam.voice().decompress(&bytes, SampleRate::INTERNAL)?; // mono f32 @ 48 kHz
  ```

  `poll` wraps `GetAvailableVoice` + `GetVoice(bWantCompressed = true, …)` with
  the deprecated uncompressed arguments passed as null/zero, growing its buffer
  on `k_EVoiceResultBufferTooSmall`; `decompress` wraps `DecompressVoice` at
  48000 and retries once with the size Steam reports if the buffer was small.
  `EVoiceResult::{NotRecording, NoData, NotInitialized, RestrictedUser, …}` map
  to typed outcomes (`NoData` is `Ok(None)`, not an error).

- **Tests (CI):** `i16` → `f32` conversion endpoints (`i16::MIN` → `-1.0`, `0` →
  `0.0`, `i16::MAX` just under `1.0`) against known values; buffer-growth loop
  on a fake that answers `BufferTooSmall` then `OK` (and a fake that answers
  `BufferTooSmall` forever must terminate with an error, not spin);
  `EVoiceResult` mapping; PTT toggling calls start/stop exactly on edges.
- **Needs a real client (two accounts):** A holds PTT and speaks; B's game
  receives packets over slice 4's transport, decompresses, and plays the PCM
  through `crcbl-audio` (a sample-side `Voice::new` per chunk is enough to hear
  it); releasing PTT stops packets within a frame; confirm Steam itself plays
  nothing (mute the sample's output → silence). Per OS as speaker at least once.
- **Exit:** audible, intelligible round trip on two OSes; `RestrictedUser` path
  exercised or named as untested.

### Slice 6 — Cloud: `SteamCloudStorage` + synced-file conflicts

- **Scope:** everything under "Cloud" above. EW requirement 5.
- **Files:** `crates/crcbl-store/src/lib.rs` (module declaration) plus a new
  `crcbl-store/src/synced.rs` for the header, shadow and classification;
  `crcbl-steam/src/cloud.rs`; `crcbl-steam/Cargo.toml` gains `crcbl-store`.
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

- **Tests (CI):** the classification table as unit tests over an in-memory
  `StorageSource` — clean, fast-forward, missing, conflict by base mismatch,
  conflict by equal generation and different CRC, corrupt header (typed error,
  never treated as empty), truncated payload (CRC catches it) — each broken once
  by hand; resolving a conflict writes `max + 1` and a second load is clean;
  `SteamCloudStorage` path validation (Steam filenames: relative, no `..`, the
  SDK's length limit) and `Unsupported` when the fake reports cloud disabled;
  `FileWrite` returning false → `StorageError`, never `Ok`.
- **Needs a real client:** does app 480 have a cloud quota? **Unverified** — the
  first step is `GetQuota`; if 480 has none, this slice's manual check waits for
  an app id of our own and says so. With quota: write on machine A, quit, launch
  on machine B → fast-forward; write offline on both, reconnect → the game shows
  `Conflict` (and record whether Steam's own dialog appeared first, and what the
  game saw after each choice); on a Deck, suspend mid-game, change the file from
  another machine, resume → `CloudFileChanged`.
- **Exit:** classification tests green; a real conflict surfaced to the game on
  at least one OS pair, or the 480-quota gap recorded.

### Slice 7 — Steam Input onto the gamepad seam, Deck text input

- **Scope:** everything under "Input" above. Needs the gamepad seam (slice 2);
  by default lands it first if absent. EW requirement 4.
- **Files:** `crcbl-steam/src/input.rs`, `crcbl-steam/assets/crcbl_pad.vdf` and
  the default controller configs; if the seam is absent,
  `crcbl-input/src/gamepad.rs` plus `Binding`/`ActionMap` changes in
  `crates/crcbl-input/src/lib.rs` as a separate commit.
- **API:**

  ```rust
  let mut pads = steam.input().init(manifest_path)?;   // ISteamInput::Init(true) + manifest
  // each frame, after steam.pump() (which ran RunFrame):
  for event in pads.poll(&steam) { action_map.gamepad_event(&event); } // crcbl_input::GamepadEvent
  let glyph: Option<PathBuf> = pads.glyph(&steam, id, PadButton::South);
  steam.utils().show_text_input(TextInputRequest { … })?; // → SteamEvent::TextInputDismissed { text: Option<String> }
  ```

- **Tests (CI):** layout of `InputAnalogActionData_t` and
  `InputDigitalActionData_t` (`pack(1)`, returned by value — table plus a
  fake-`Lib` function that returns a known value, read back through the declared
  signature, so an ABI mismatch between the declaration and a real by-value
  return shows up as garbage in the test); action-data → snapshot mapping (stick
  Y sign convention matching `virtual_stick`'s +Y up, triggers 0..1, digital
  bitset) from fixtures; connect/disconnect → `GamepadId` stability for a
  re-plugged handle; **one owner per pad**: with the Steam backend live, a
  native-backend device with vendor `0x28DE` is skipped — test red when the
  filter is removed; `ActionMap` resolution of `PadButton`/`PadStick` bindings
  identical whether the event came from the Steam backend or a hand-built
  `GamepadEvent` (that identity is requirement 4, stated as a test).
- **Needs a real client:** Deck (Linux) as the primary: every `PadButton` and
  axis reaches the `ActionMap`; remap in Steam's configurator and see the
  remapped action; no double input with a native backend present; the Deck
  keyboard fills a text field via `TextInputDismissed`; glyphs load. Windows and
  macOS with a DualSense and an Xbox pad: same checks; confirm Steam Input is
  actually active for the app (the Steam overlay's controller settings show the
  manifest's action set).
- **Exit:** Deck run recorded end to end; Windows and macOS runs recorded or
  named as gaps; the vendor filter test proven red once.

### Slice 8 — The engine `Loop` limb

- **Scope:** `Loop` owns an `Option<Steam>` (umbrella `steam` feature), pumps it
  in `frame_body` under its own trace span beside `shell.pump`, routes
  `OverlayActivated { active: true }` through `lose_focus`, and exposes the
  drained events to the game (a `HostedGame` hook with a no-op default). Lands
  after the second app (or EW) repeats the pattern — by default right after
  slice 7, since EW plus `apps/sandbox` are two callers.
- **Files:** `crates/crcbl/src/engine.rs` (or the module the loop has moved to
  by then — `docs/backlog.md` records that file as oversized, so this slice adds
  the limb as its own module rather than growing it), `crates/crcbl/Cargo.toml`.
- **Tests (CI):** the loop with a fake `Steam` event source (the loop takes a
  trait object for "an event source it pumps", implemented by `Steam` and by a
  test double inside `crcbl`'s tests — the double is not a fake Steam, it is a
  scripted event source, and cannot sign anyone in): an overlay-open event
  pauses and releases held keys exactly as a focus loss does (the existing
  focus-loss test, parameterised over both causes); overlay-close does not
  unpause by itself (resume stays the player's action, matching focus).
- **Needs a real client:** Shift+Tab in a `Loop`-driven sample pauses it and
  releases held input on each OS.
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

- **Callback packing differs by OS.** `steamclientpublic.h`: Linux, macOS and
  FreeBSD `#pragma pack(push, 4)`; Windows `pack(push, 8)`. Struct tables must
  disagree across platforms for any struct with a `u64` after an odd count of
  `u32`s — if they all agree, suspect the table.
- **And some structs ignore that rule.** `steamnetworkingtypes.h` opens with the
  same 4/8 selection but wraps `SteamNetworkingIPAddr` and
  `SteamNetworkingIdentity` in `#pragma pack(push,1)`; `isteaminput.h` wraps
  `InputAnalogActionData_t` and `InputDigitalActionData_t` in
  `#pragma pack( push, 1 )`. Read the pragma in force at each struct, never
  assume the file's.
- **Struct returned by value from a packed type.** `GetAnalogActionData` returns
  a 1-packed struct by value. Rust's `extern "C"` must classify it exactly as
  the C compiler does (on SysV x86-64 an unaligned field forces a memory return;
  on Windows x64 a size that is not 1, 2, 4 or 8 bytes does). A matching
  `#[repr(C, packed)]` is correct in principle; the fake-`Lib` return test plus
  the real-Deck check in slice 7 are what prove it.
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
  `const SteamNetworkingIdentity *` parameter; `GetVoice` still carries four
  deprecated uncompressed arguments. Bindings from memory corrupt the stack.
  Every declaration carries its SDK version; the drift gate re-checks the lot.
- **Received networking messages must be released exactly once** — copied out
  and released immediately in `recv`.
- **Tickets, lobbies, recordings and connections leak** unless something ends
  them. RAII on the owning value; not `Drop` of `Steam`.
- **Steam Input's virtual pad doubles input** through the native backends unless
  filtered (slice 7).
- **`steam_appid.txt` inverts a guard.** With it present,
  `RestartAppIfNecessary` returns false regardless — correct for dev,
  catastrophic if shipped. `.gitignore` plus slice 15's package check.
- **App id 480 is shared.** Mechanisms only; leave achievements as found.

## Risks and open questions

Each is labelled with what it blocks and whether it needs the user.

- **R1 — `ISteamRemoteStorage` thread safety (blocks: slice 6's `Send` claim).**
  `StorageSource: Send` forces `SteamCloudStorage: Send`; Valve's general
  statement covers it, no interface-specific statement was found. Mitigation:
  `ReleaseCurrentThreadMemory` after off-thread use, and slice 6 documents the
  inference in the `unsafe impl`. If review rejects it, the fallback is a
  `SteamCloudStorage` that forwards calls to the pump thread through a queue and
  returns `StorageError::Pending` until answered — the browser backends' shape.
- **R2 — Steam's launch-time conflict dialog (slice 6).** It can still pre-empt
  the game; our protocol then sees the chosen file. What the game observes after
  each dialog choice is recorded in slice 6's manual run, not assumed.
- **R3 — App 480 capabilities (slices 6, 9, 14, 15).** Cloud quota, SpaceWar's
  achievements/leaderboard/item definitions, and rich-presence localisation
  under 480 are believed, not verified. Each slice's first manual step checks,
  and a missing capability waits for our own app id rather than faking it.
- **R4 — Overlay over our own windowing (every slice).** Unverified on every
  shell backend × GPU backend; recorded per slice.
- **R5 — Multi-session server (EW requirement 1).** Outside this topic; slice 4
  cannot prove four players until it exists. **User decision:** whether that
  work is scheduled with the Steam slices or EW's host fans out itself.
- **R6 — Gamepad seam ownership (slice 7).** Two plans want to define it. The
  sketch here is the proposal; topic 19's evdev slice must agree.
- **R7 — Linux shipping glibc floor (slice 15).** Binaries built on
  `ubuntu-latest` will not start inside `sniper`. Needs a container build.
- **R8 — macOS signing/entitlements for the dylib and overlay (slice 15).**
  Unverified.
- **R9 — Accessor versions move.** The table above is the mirror's; the first
  real SDK may differ. The drift gate is the answer; until it has run once,
  nothing in `versions.rs` is trusted.
- **Needs the user, not blocking any slice before 15: an app id of our own.**
  Achievement schemas, cloud quota, rich-presence tokens, Steam Input default
  configurations and depots are per-app. Until then 480.

## Defaulted decisions

The user was away; each of these was picked by this plan and is the user's to
overturn. The four ratified 2026-09-06 are listed first for completeness.

| Decision                                                 | Default taken                                                                                           | Alternative, and its cost                                                                                                                       |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Binding route (ratified)                                 | hand-written flat-API declarations, drift gate                                                          | `steamworks-rs`: new dependency, link-time death without the library                                                                            |
| Cloud (ratified, then **overridden by EW**)              | `ISteamRemoteStorage` backend required, because Auto-Cloud cannot surface a conflict                    | Auto-Cloud only: zero code, but EW requirement 5 unmet                                                                                          |
| Steam Input (ratified "if Deck targeted" → now in scope) | Steam Input onto a shared gamepad seam, one owner per pad                                               | rely on Steam's XInput/evdev emulation only: no Steam code, but no Deck glyphs/remap awareness, and EW 4's "same events" only holds by accident |
| First app id (ratified)                                  | 480 for every slice until slice 15                                                                      | own app id: partner fee and a product decision                                                                                                  |
| Loading                                                  | runtime `dlopen`/`LoadLibraryExW`, absolute paths, exe dir then `CRCBL_STEAM_SDK`                       | link-time: CI cannot build                                                                                                                      |
| SDK in CI                                                | never; drift gate local-only                                                                            | CI fetches `steam_api.json` (from a secret or the Steamworks.NET mirror): drift checked on every push, licence posture of the mirror unclear    |
| Drift-gate JSON parsing                                  | `serde_json` as a dev-dependency (already in `Cargo.lock`)                                              | a `tools/` `.mjs` script: no new edge, a second language in the gate                                                                            |
| `steam_appid.txt`                                        | crate never writes it or sets env vars; error message says what is missing                              | write it or `set_var`: convenient, but a side effect in the user's cwd or an `unsafe` env write with threads live                               |
| 32-bit and `aarch64-linux`                               | 32-bit out of scope; `linuxarm64` path listed but unverified                                            | support 32-bit: a build and test matrix nothing else in the workspace has                                                                       |
| Gamepad seam if topic 19 has not landed                  | slice 7 lands it, as its own commit                                                                     | wait for topic 19: EW requirement 4 waits too                                                                                                   |
| Slice order                                              | EW's hard requirements first (3–7), then engine loop, then the rest                                     | the earlier order (stats before networking): EW waits                                                                                           |
| Microtransactions                                        | declined (needs a server holding a publisher key)                                                       | build it: hosted infrastructure the project does not run                                                                                        |
| `Send` surfaces                                          | `SteamTransport` and `SteamCloudStorage` are `Send` via a shared `Arc<Client>`; everything else `!Send` | all `!Send`: they could not implement `Transport`/`StorageSource` at all                                                                        |

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
  (2026-09-22: `steam_api_flat.h` for every accessor and signature quoted,
  `isteamuser.h` for voice, `steamnetworkingtypes.h` and
  `isteamnetworkingsockets.h` for message limits, send flags, end-reason ranges
  and packing, `isteaminput.h` for the action-data packing).
- [SDK 1.63 release announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/627817201164877826),
  [SDK 1.61 announcement](https://steamcommunity.com/groups/steamworks/announcements/detail/4480612432780198328),
  [SDK 1.62 patch notes](https://steamdb.info/patchnotes/17946746/).
- [Noxime/steamworks-rs](https://github.com/Noxime/steamworks-rs) — README,
  `steamworks-sys/build.rs` and `Cargo.toml`, for the rejected option.
