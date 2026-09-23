//! Steam Cloud against a real client, through the public API only. Never run
//! by CI: `#[ignore]`d, and run by hand as `tests/smoke.rs` is (Steam running
//! and logged in, the redistributable reachable, `steam_appid.txt` in the
//! crate's directory):
//!
//! ```text
//! cargo test -p crcbl-steam --test cloud_smoke -- --ignored --nocapture
//! ```
//!
//! It prints whether cloud is on and the quota — the first thing to learn
//! about an app id, since app 480 may have none — then writes, reads, lists
//! and deletes one file, and loads and saves a synced file over it. With no
//! cloud for the app it says so and stops.
//!
//! Conflicts need two machines and are a manual step: see
//! `docs/plan/42-steam.md`, slice 6, "Needs a real client".

#![cfg(all(
    target_pointer_width = "64",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]

use std::path::Path;

use crcbl_steam::{AppId, Steam, SteamCloudStorage};
use crcbl_store::{MemoryStorage, StorageError, StorageSource, synced::SyncedFile};

#[test]
#[ignore = "needs a running Steam client, the redistributable and steam_appid.txt"]
fn write_read_list_and_delete_one_cloud_file() {
    let steam = Steam::init(AppId(480)).unwrap_or_else(|err| panic!("Steam::init: {err}"));
    let cloud = match SteamCloudStorage::new(&steam) {
        Ok(cloud) => cloud,
        Err(StorageError::Unsupported(why)) => {
            println!("no Steam Cloud here: {why}");
            return;
        }
        Err(err) => panic!("SteamCloudStorage::new: {err}"),
    };
    println!("quota {:?}", cloud.quota());

    let path = Path::new("crcbl-cloud-smoke.bin");
    cloud.write(path, b"crcbl cloud smoke").expect("FileWrite");
    assert_eq!(cloud.read(path).expect("FileRead"), b"crcbl cloud smoke");
    let listed = cloud.list(Path::new(".")).expect("the file list");
    println!("listed {listed:?}");
    assert!(listed.iter().any(|entry| entry == path));
    cloud.delete(path).expect("FileDelete");
    assert!(!cloud.exists(path));

    let mut synced = SyncedFile::new(
        Box::new(SteamCloudStorage::new(&steam).expect("cloud")),
        Box::new(MemoryStorage::new()),
        "crcbl-synced-smoke.bin",
    );
    println!("first load: {:?}", synced.load().expect("load"));
    synced.save(b"saved by the smoke test").expect("save");
    println!("second load: {:?}", synced.load().expect("load"));
    cloud
        .delete(Path::new("crcbl-synced-smoke.bin"))
        .expect("FileDelete");
}
