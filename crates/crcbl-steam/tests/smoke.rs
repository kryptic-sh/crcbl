//! Against a real Steam client, through the public API only. Never run by CI,
//! which has no Steam: `#[ignore]`d, and run by hand on each OS with the
//! Steam client running and logged in, the redistributable reachable (beside
//! the test binary, or via `CRCBL_STEAM_SDK`), and `steam_appid.txt`
//! containing `480` in the working directory — the crate's own directory,
//! under `cargo test`:
//!
//! ```text
//! cd crates/crcbl-steam && echo 480 > steam_appid.txt
//! CRCBL_STEAM_SDK=/path/to/sdk cargo test -p crcbl-steam --test smoke -- --ignored --nocapture
//! ```
//!
//! App 480 is shared by every Steamworks developer, so this asserts
//! mechanisms — init succeeds, the pipe drains cleanly — and never a value
//! read back from 480. The printed `SteamId` should be the same across two
//! runs on one account; compare them by eye.

#![cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]

use crcbl_steam::{AppId, Steam};

#[test]
#[ignore = "needs a running Steam client, the redistributable and steam_appid.txt"]
fn init_pump_and_shut_down_under_app_480() {
    let mut steam = match Steam::init(AppId(480)) {
        Ok(steam) => steam,
        Err(err) => panic!("Steam::init failed: {err}"),
    };
    let me = steam.user().steam_id();
    println!(
        "SteamId {}, logged on {}, app {:?}, hardware {:?}",
        me.0,
        steam.user().logged_on(),
        steam.utils().app_id(),
        steam.utils().steam_hardware()
    );
    assert_ne!(me.0, 0, "a logged-in user has a non-zero SteamId");
    for _ in 0..300 {
        steam.pump();
        for event in steam.events() {
            println!("{event:?}");
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let diagnostics = steam.diagnostics();
    println!("{diagnostics:?}");
    assert_eq!(diagnostics.decode_mismatches, 0);
    assert_eq!(diagnostics.null_payloads, 0);
    drop(steam);
}
