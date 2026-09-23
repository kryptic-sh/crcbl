//! `SteamCloudStorage` over the fake library's cloud.

use crcbl_store::MemoryStorage;
use crcbl_store::synced::{SyncOutcome, SyncedFile};

use super::*;
use crate::{
    AppId, SteamEvent,
    client::init_on,
    testing::{self, FakeMsg, script},
};

fn steam() -> Steam {
    init_on(testing::fake_lib(), AppId(480)).unwrap()
}

fn calls() -> u32 {
    script(|s| s.cloud.calls)
}

#[test]
fn cloud_off_for_the_account_or_the_app_is_unsupported() {
    for (account, app) in [(false, true), (true, false), (false, false)] {
        let steam = steam();
        script(|s| {
            s.cloud.account_enabled = account;
            s.cloud.app_enabled = app;
        });
        assert!(
            matches!(
                SteamCloudStorage::new(&steam),
                Err(StorageError::Unsupported(_))
            ),
            "account {account}, app {app}"
        );
    }
    script(|s| {
        s.cloud.account_enabled = true;
        s.cloud.app_enabled = true;
    });
    assert!(SteamCloudStorage::new(&steam()).is_ok());
}

#[test]
fn a_file_round_trips_through_one_batched_write() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    let path = Path::new("saves/profile.bin");
    assert!(!cloud.exists(path));
    assert!(matches!(cloud.read(path), Err(StorageError::NotFound(_))));

    cloud.write(path, b"payload").unwrap();
    assert_eq!(
        script(|s| s.cloud.log.clone()),
        ["begin", "write saves/profile.bin", "end"]
    );
    assert!(cloud.exists(path));
    assert_eq!(cloud.read(path).unwrap(), b"payload");
    cloud.write(Path::new("saves/empty.bin"), b"").unwrap();
    assert_eq!(cloud.read(Path::new("saves/empty.bin")).unwrap(), b"");

    cloud.write(Path::new("top.bin"), b"top").unwrap();
    assert_eq!(
        cloud.list(Path::new("saves")).unwrap(),
        [
            PathBuf::from("saves").join("empty.bin"),
            PathBuf::from("saves").join("profile.bin")
        ]
    );
    assert_eq!(
        cloud.list(Path::new(".")).unwrap(),
        [PathBuf::from("top.bin")]
    );

    cloud.delete(path).unwrap();
    assert!(!cloud.exists(path));
    assert!(matches!(cloud.delete(path), Err(StorageError::NotFound(_))));
}

#[test]
fn a_write_steam_refuses_is_an_error_never_ok() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    script(|s| s.cloud.refuse_writes = true);
    assert!(matches!(
        cloud.write(Path::new("profile.bin"), b"lost?"),
        Err(StorageError::Other(_))
    ));
    assert_eq!(
        script(|s| s.cloud.log.clone()),
        ["begin", "write profile.bin", "end"],
        "the batch is still closed"
    );
}

#[test]
fn a_short_read_is_an_error() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    cloud.write(Path::new("profile.bin"), b"four").unwrap();
    script(|s| s.cloud.short_reads = true);
    assert!(matches!(
        cloud.read(Path::new("profile.bin")),
        Err(StorageError::Other(_))
    ));
}

#[test]
fn a_path_steam_cannot_name_is_refused_before_any_call() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    let longest = "a".repeat(MAX_CLOUD_PATH_BYTES);
    let too_long = "a".repeat(MAX_CLOUD_PATH_BYTES + 1);
    let absolute = std::env::current_dir().unwrap().join("profile.bin");
    let before = calls();
    for bad in [
        Path::new("../escape.bin"),
        Path::new("saves/../../escape.bin"),
        absolute.as_path(),
        Path::new(&too_long),
        Path::new(""),
        Path::new("nul\0inside"),
    ] {
        assert!(
            matches!(cloud.write(bad, b"x"), Err(StorageError::InvalidPath(_))),
            "{bad:?}"
        );
        assert!(
            matches!(cloud.read(bad), Err(StorageError::InvalidPath(_))),
            "{bad:?}"
        );
        assert!(!cloud.exists(bad), "{bad:?}");
    }
    assert_eq!(calls(), before, "Steam was called for a refused path");
    cloud.write(Path::new(&longest), b"fits").unwrap();
    cloud.write(Path::new("./saves/./a.bin"), b"dots").unwrap();
    assert!(script(|s| s.cloud.files.contains_key("saves/a.bin")));
}

// Not under Miri: a 100 MiB buffer is minutes of interpretation, and the
// check it proves is plain arithmetic.
#[cfg(not(miri))]
#[test]
fn a_file_over_the_chunk_limit_is_refused_before_any_call() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    let before = calls();
    let huge = vec![0_u8; MAX_CLOUD_FILE_BYTES + 1];
    assert!(matches!(
        cloud.write(Path::new("huge.bin"), &huge),
        Err(StorageError::LimitExceeded(_))
    ));
    assert_eq!(calls(), before);
}

#[test]
fn off_the_pump_thread_nothing_calls_steam() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    cloud.write(Path::new("profile.bin"), b"here").unwrap();
    let before = calls();
    let calls_there = std::thread::spawn(move || {
        let path = Path::new("profile.bin");
        assert!(matches!(
            cloud.read(path),
            Err(StorageError::Unsupported(_))
        ));
        assert!(matches!(
            cloud.write(path, b"there"),
            Err(StorageError::Unsupported(_))
        ));
        assert!(matches!(
            cloud.delete(path),
            Err(StorageError::Unsupported(_))
        ));
        assert!(matches!(
            cloud.list(Path::new(".")),
            Err(StorageError::Unsupported(_))
        ));
        assert!(!cloud.exists(path));
        assert_eq!(cloud.quota(), None);
        script(|s| s.cloud.calls)
    })
    .join()
    .unwrap();
    assert_eq!(calls_there, 0, "a Steam call was made off the pump thread");
    assert_eq!(calls(), before, "nor on this thread's behalf");
    assert_eq!(
        script(|s| s.cloud.files.get("profile.bin").cloned()),
        Some(b"here".to_vec())
    );
}

#[test]
fn the_quota_is_read_when_steam_gives_one() {
    let steam = steam();
    let cloud = SteamCloudStorage::new(&steam).unwrap();
    assert_eq!(cloud.quota(), None);
    script(|s| s.cloud.quota = Some((1000, 250)));
    assert_eq!(
        cloud.quota(),
        Some(CloudQuota {
            total: 1000,
            available: 250
        })
    );
}

#[test]
fn a_local_file_change_is_an_event_per_changed_file() {
    let mut steam = steam();
    script(|s| {
        s.cloud.changes = vec![
            ("profile.bin".into(), 1, 2),
            ("saves/slot1.bin".into(), 2, 2),
        ];
        s.queue.push_back(FakeMsg::payload(1333, vec![0]));
    });
    steam.pump();
    let events: Vec<_> = steam.events().collect();
    assert_eq!(
        events,
        [
            SteamEvent::CloudFileChanged {
                path: "profile.bin".into()
            },
            SteamEvent::CloudFileChanged {
                path: "saves/slot1.bin".into()
            },
        ]
    );
    assert_eq!(steam.diagnostics().decode_mismatches, 0);
}

#[test]
fn a_synced_file_keeps_its_versions_in_steam_cloud() {
    let steam = steam();
    let mut profile = SyncedFile::new(
        Box::new(SteamCloudStorage::new(&steam).unwrap()),
        Box::new(MemoryStorage::new()),
        "profile.bin",
    );
    assert_eq!(profile.load().unwrap(), SyncOutcome::Missing);
    profile.save(b"level 3").unwrap();
    assert_eq!(
        profile.load().unwrap(),
        SyncOutcome::Clean(b"level 3".to_vec())
    );

    // Another device's version arrives, as Steam syncs it down.
    let mut other = SyncedFile::new(
        Box::new(SteamCloudStorage::new(&steam).unwrap()),
        Box::new(MemoryStorage::new()),
        "profile.bin",
    );
    other.load().unwrap();
    other.save(b"level 4").unwrap();
    assert_eq!(
        profile.load().unwrap(),
        SyncOutcome::FastForwarded(b"level 4".to_vec())
    );
}
