//! The load table, row by row, over in-memory clouds shared between
//! simulated devices.

use std::sync::{Arc, Mutex};

use super::*;
use crate::MemoryStorage;

const FILE: &str = "profile.bin";

/// A `MemoryStorage` several devices hold at once, as they share one cloud;
/// its writes can be made to fail, as a cloud's can.
#[derive(Debug, Clone, Default)]
struct Shared {
    storage: Arc<Mutex<MemoryStorage>>,
    refuse_writes: Arc<Mutex<bool>>,
}

impl Shared {
    fn with<R>(&self, f: impl FnOnce(&MemoryStorage) -> R) -> R {
        f(&self.storage.lock().expect("not poisoned"))
    }

    fn refuse_writes(&self, refuse: bool) {
        *self.refuse_writes.lock().expect("not poisoned") = refuse;
    }

    fn raw(&self) -> Vec<u8> {
        self.with(|s| s.read(Path::new(FILE))).expect("the file")
    }

    fn put(&self, bytes: &[u8]) {
        self.with(|s| s.write(Path::new(FILE), bytes))
            .expect("a write");
    }
}

impl StorageSource for Shared {
    fn read(&self, path: &Path) -> Result<Vec<u8>, StorageError> {
        self.with(|s| s.read(path))
    }

    fn write(&self, path: &Path, data: &[u8]) -> Result<(), StorageError> {
        if *self.refuse_writes.lock().expect("not poisoned") {
            return Err(StorageError::Other("the cloud refused the write".into()));
        }
        self.with(|s| s.write(path, data))
    }

    fn delete(&self, path: &Path) -> Result<(), StorageError> {
        self.with(|s| s.delete(path))
    }

    fn exists(&self, path: &Path) -> bool {
        self.with(|s| s.exists(path))
    }

    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, StorageError> {
        self.with(|s| s.list(dir))
    }
}

/// A device: its view of the shared cloud, and a shadow of its own.
fn device(cloud: &Shared) -> SyncedFile {
    SyncedFile::new(
        Box::new(cloud.clone()),
        Box::new(MemoryStorage::new()),
        FILE,
    )
}

fn clean(bytes: &[u8]) -> SyncOutcome {
    SyncOutcome::Clean(bytes.to_vec())
}

fn forwarded(bytes: &[u8]) -> SyncOutcome {
    SyncOutcome::FastForwarded(bytes.to_vec())
}

fn conflict(local: &[u8], remote: &[u8]) -> SyncOutcome {
    SyncOutcome::Conflict {
        local: local.to_vec(),
        remote: remote.to_vec(),
    }
}

/// The cloud's current version, parsed.
fn cloud_version(cloud: &Shared) -> Parsed {
    Parsed::parse(cloud.raw()).expect("a valid synced file")
}

/// Two devices that both hold version `v1` of the file.
fn two_devices_at_v1() -> (Shared, SyncedFile, SyncedFile) {
    let cloud = Shared::default();
    let mut a = device(&cloud);
    let mut b = device(&cloud);
    assert_eq!(a.load().unwrap(), SyncOutcome::Missing);
    a.save(b"v1").unwrap();
    assert_eq!(a.load().unwrap(), clean(b"v1"));
    assert_eq!(b.load().unwrap(), forwarded(b"v1"));
    (cloud, a, b)
}

#[test]
fn nothing_anywhere_is_missing() {
    let cloud = Shared::default();
    assert_eq!(device(&cloud).load().unwrap(), SyncOutcome::Missing);
}

#[test]
fn a_devices_own_write_loads_clean_and_a_new_device_fast_forwards() {
    let cloud = Shared::default();
    let mut a = device(&cloud);
    a.load().unwrap();
    a.save(b"hello").unwrap();
    assert_eq!(a.load().unwrap(), clean(b"hello"));
    assert_eq!(a.load().unwrap(), clean(b"hello"), "and stays clean");

    let mut b = device(&cloud);
    assert_eq!(b.load().unwrap(), forwarded(b"hello"));
    assert_eq!(b.load().unwrap(), clean(b"hello"));
}

#[test]
fn a_device_that_changed_nothing_fast_forwards_to_each_new_version() {
    let (_cloud, mut a, mut b) = two_devices_at_v1();
    a.save(b"v2").unwrap();
    assert_eq!(b.load().unwrap(), forwarded(b"v2"));
    a.load().unwrap();
    a.save(b"v3").unwrap();
    assert_eq!(b.load().unwrap(), forwarded(b"v3"));
}

#[test]
fn a_write_built_on_this_devices_unconfirmed_write_fast_forwards() {
    let (_cloud, mut a, mut b) = two_devices_at_v1();
    b.save(b"b").unwrap();
    // `a` sees `b`'s write and builds on it before `b` loads again.
    assert_eq!(a.load().unwrap(), forwarded(b"b"));
    a.save(b"a on b").unwrap();
    assert_eq!(b.load().unwrap(), forwarded(b"a on b"));
}

#[test]
fn equal_generations_with_different_payloads_conflict() {
    let (cloud, mut a, mut b) = two_devices_at_v1();
    a.save(b"a").unwrap();
    b.save(b"b").unwrap();
    // Both wrote generation 2 on top of v1; the cloud kept `b`'s.
    assert_eq!(cloud_version(&cloud).version.generation, 2);
    assert_eq!(a.load().unwrap(), conflict(b"a", b"b"));
    assert_eq!(
        b.load().unwrap(),
        clean(b"b"),
        "b's own write is in the cloud"
    );
}

#[test]
fn a_cloud_version_on_another_base_conflicts() {
    let (cloud, mut a, mut b) = two_devices_at_v1();
    a.save(b"a").unwrap();
    b.save(b"b1").unwrap();
    b.load().unwrap();
    b.save(b"b2").unwrap();
    // `b2` is generation 3 on top of `b1`, which `a`'s write never saw.
    assert_eq!(cloud_version(&cloud).version.generation, 3);
    assert_eq!(a.load().unwrap(), conflict(b"a", b"b2"));
}

#[test]
fn resolving_writes_above_both_sides_and_the_next_load_is_clean() {
    for (choice, kept) in [
        (Resolution::KeepLocal, &b"a"[..]),
        (Resolution::KeepRemote, &b"b2"[..]),
    ] {
        let (cloud, mut a, mut b) = two_devices_at_v1();
        a.save(b"a").unwrap();
        b.save(b"b1").unwrap();
        b.load().unwrap();
        b.save(b"b2").unwrap();
        assert_eq!(a.load().unwrap(), conflict(b"a", b"b2"));
        assert!(matches!(a.save(b"x"), Err(SyncError::Unresolved)));

        assert_eq!(a.resolve(choice).unwrap(), kept);
        let resolved = cloud_version(&cloud);
        assert_eq!(resolved.version.generation, 4, "max(2, 3) + 1");
        assert_eq!(resolved.payload(), kept);
        assert_eq!(a.load().unwrap(), clean(kept), "{choice:?}");
        // The resolution was written on top of `b2`, so `b` just moves on.
        assert_eq!(b.load().unwrap(), forwarded(kept), "{choice:?}");
        assert!(matches!(a.resolve(choice), Err(SyncError::NoConflict)));
    }
}

#[test]
fn a_save_needs_a_load_first() {
    let cloud = Shared::default();
    let mut a = device(&cloud);
    assert!(matches!(a.save(b"blind"), Err(SyncError::NotLoaded)));
    assert!(!cloud.with(|s| s.exists(Path::new(FILE))));
}

#[test]
fn a_write_the_cloud_never_took_is_sent_again_on_load() {
    let cloud = Shared::default();
    let mut a = device(&cloud);
    a.load().unwrap();
    cloud.refuse_writes(true);
    assert!(matches!(a.save(b"first"), Err(SyncError::Storage(_))));
    cloud.refuse_writes(false);
    assert_eq!(a.load().unwrap(), clean(b"first"));
    assert_eq!(cloud_version(&cloud).payload(), b"first");

    // And over an older version: the cloud still holds what it replaced.
    cloud.refuse_writes(true);
    assert!(a.save(b"second").is_err());
    cloud.refuse_writes(false);
    assert_eq!(a.load().unwrap(), clean(b"second"));
    assert_eq!(cloud_version(&cloud).payload(), b"second");
}

#[test]
fn a_header_that_does_not_parse_is_a_typed_error_never_empty() {
    let mut good = Parsed::build(1, None, b"payload").bytes;
    let mut wrong_format = good.clone();
    wrong_format[4] = 2;
    for (bytes, expected) in [
        (b"CRSF".to_vec(), Corruption::ShortHeader),
        (vec![0; HEADER_BYTES + 3], Corruption::NotSynced),
        (wrong_format, Corruption::Format(2)),
    ] {
        let cloud = Shared::default();
        cloud.put(&bytes);
        let mut a = device(&cloud);
        match a.load() {
            Err(SyncError::Corrupt { corruption, .. }) => assert_eq!(corruption, expected),
            other => panic!("expected {expected:?}, got {other:?}"),
        }
        // And nothing may be saved over it on the strength of that load.
        assert!(matches!(a.save(b"over it"), Err(SyncError::NotLoaded)));
        assert_eq!(cloud.raw(), bytes);
    }
    good.truncate(good.len() - 1);
    let cloud = Shared::default();
    cloud.put(&good);
    assert!(matches!(
        device(&cloud).load(),
        Err(SyncError::Corrupt {
            corruption: Corruption::Length {
                declared: 7,
                present: 6
            },
            ..
        })
    ));
}

#[test]
fn a_damaged_byte_anywhere_fails_the_checksum() {
    let good = Parsed::build(5, None, b"payload").bytes;
    // One byte in the payload, and one in the generation field.
    for at in [HEADER_BYTES + 2, 8] {
        let mut bytes = good.clone();
        bytes[at] ^= 0x01;
        let cloud = Shared::default();
        cloud.put(&bytes);
        assert!(
            matches!(
                device(&cloud).load(),
                Err(SyncError::Corrupt {
                    corruption: Corruption::Checksum,
                    ..
                })
            ),
            "byte {at}"
        );
    }
}

#[test]
fn a_version_round_trips_through_its_bytes() {
    let base = Version {
        generation: 3,
        crc: 0xDEAD_BEEF,
    };
    let built = Parsed::build(4, Some(base), b"data");
    let parsed = Parsed::parse(built.bytes.clone()).unwrap();
    assert_eq!(parsed.version, built.version);
    assert_eq!(parsed.base, Some(base));
    assert_eq!(parsed.payload(), b"data");
    assert_eq!(built.bytes.len(), HEADER_BYTES + 4);
    assert_eq!(
        Parsed::parse(Parsed::build(1, None, b"").bytes)
            .unwrap()
            .base,
        None
    );
}
