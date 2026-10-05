//! The chain against a real version-2 save, and every way it refuses one.

use std::path::Path;

use super::super::tests::{sealed, written};
use super::super::{
    CHECKSUM_SIZE, MIN_SECTOR_ENTRY_SIZE, SECTOR_ID_SIZE, SaveHeader, SaveReader, SaveWriter,
    SectorSave, decode, version_of,
};
use super::*;
use crate::{MemoryStorage, StorageError, StorageSource};
use crcbl_core::TickId;
use crcbl_net::types::SectorId;

/// A save written by the version-2 writer, before version 3 existed: tick
/// 9 001, 3 723.25 s of play, and three sectors — four bytes at the origin,
/// five at a sector with negative coordinates, and an empty one.
const V2_FIXTURE: &[u8] = include_bytes!("../../../tests/fixtures/save-v2.crb");

/// The fixture's sectors, as the version-2 writer was handed them.
fn fixture_sectors() -> Vec<(SectorId, Vec<u8>)> {
    vec![
        (SectorId::ZERO, vec![1, 2, 3, 4]),
        (
            SectorId { x: -1, y: 2, z: -3 },
            vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00],
        ),
        (SectorId { x: 4, y: 0, z: 0 }, Vec::new()),
    ]
}

/// Version 2's header: the fixed fields, then the sector count.
const V2_HEADER_SIZE: usize = FIXED_HEADER_SIZE + 4;

/// Where the fixture's second sector's data length is: after the header, the
/// first sector's entry and its four bytes, and the second sector's id.
const SECOND_LEN_AT: usize = V2_HEADER_SIZE + MIN_SECTOR_ENTRY_SIZE + 4 + SECTOR_ID_SIZE;

/// The fixture's body, everything before its checksum.
fn fixture_body() -> &'static [u8] {
    &V2_FIXTURE[..V2_FIXTURE.len() - CHECKSUM_SIZE]
}

/// The fixture's body with `edit` made to it, sealed with a checksum that
/// matches: a file the checksum cannot catch.
fn resealed(edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut body = fixture_body().to_vec();
    edit(&mut body);
    sealed(body)
}

/// The fixture's body at `version`, resealed.
fn at_version(version: u16) -> Vec<u8> {
    resealed(|body| body[VERSION_AT..VERSION_AT + 2].copy_from_slice(&version.to_le_bytes()))
}

/// **The version-2 fixture opens through the migration**, with every field
/// of its header and every sector as the version-2 writer was handed them.
#[test]
fn the_v2_fixture_opens_through_the_migration_with_every_field_and_sector() {
    assert_eq!(
        version_of(V2_FIXTURE),
        Some(2),
        "the fixture is a version-2 save"
    );
    let storage = MemoryStorage::new();
    let path = Path::new("v2.crb");
    storage
        .write(path, V2_FIXTURE)
        .expect("a memory storage takes every write");
    let read = SaveReader::open(&storage, path)
        .expect("a version-2 save opens")
        .into_data();

    assert_eq!(read.format_version, 2);
    assert!(read.checksum_valid);
    assert_eq!(read.header.tick, TickId::from_raw(9_001));
    assert_eq!(read.header.playtime_secs.to_bits(), 3_723.25_f64.to_bits());
    assert_eq!(
        read.header.engine_version, None,
        "version 2 did not record one"
    );
    assert_eq!(read.header.scene, None);
    let sectors: Vec<(SectorId, Vec<u8>)> = read
        .sectors
        .into_iter()
        .map(|sector| (sector.sector_id, sector.snapshot_data))
        .collect();
    assert_eq!(sectors, fixture_sectors());
}

/// **`migrate` turns the fixture into exactly the body today's writer writes**
/// for the same save with no engine version and no scene — byte for byte, so
/// the step moved every field to where the current reader looks for it.
#[test]
fn the_v2_fixture_migrates_to_exactly_what_the_writer_writes_today() {
    let migrated = to_current(2, fixture_body()).expect("a version-2 body migrates");

    let mut writer = SaveWriter::new(SaveHeader {
        engine_version: None,
        ..SaveHeader::new(TickId::from_raw(9_001), 3_723.25)
    });
    for (sector_id, snapshot_data) in fixture_sectors() {
        writer.add_sector(SectorSave {
            sector_id,
            snapshot_data,
        });
    }
    assert_eq!(
        *migrated,
        *writer.encode_body().expect("a valid header"),
        "the migrated body is not the current layout"
    );

    // …and a current body goes through untouched.
    let current = writer.encode_body().expect("a valid header");
    assert!(matches!(
        to_current(SAVE_FORMAT_VERSION, &current),
        Ok(Cow::Borrowed(_))
    ));
}

/// **A migrated save is written back at the current version**: migration is
/// in memory, and the writer only ever writes [`SAVE_FORMAT_VERSION`].
#[test]
fn a_migrated_save_is_written_back_at_the_current_version() {
    let read = decode(V2_FIXTURE).expect("a version-2 save opens");
    let mut writer = SaveWriter::new(read.header.clone());
    for sector in read.sectors.clone() {
        writer.add_sector(sector);
    }
    let rewritten = written(&writer).expect("a valid header");
    assert_eq!(version_of(&rewritten), Some(SAVE_FORMAT_VERSION));

    let reread = decode(&rewritten).expect("the save this build just wrote");
    assert_eq!(reread.format_version, SAVE_FORMAT_VERSION);
    assert_eq!(reread.header, read.header);
    assert_eq!(reread.sectors.len(), read.sectors.len());
}

/// **A corrupt version-2 save is refused, not misread**: a flipped bit fails
/// the checksum taken over the bytes as written, and a damaged sector length
/// under a checksum that matches is refused by the current reader after the
/// step, as a fresh file's would be.
#[test]
fn a_corrupt_v2_save_is_refused_not_misread() {
    let mut flipped = V2_FIXTURE.to_vec();
    flipped[SECOND_LEN_AT + 4] ^= 0x01;
    let storage = MemoryStorage::new();
    let path = Path::new("flipped.crb");
    storage
        .write(path, &flipped)
        .expect("a memory storage takes every write");
    let error = SaveReader::open(&storage, path).expect_err("a corrupt save was opened");
    assert!(error.to_string().contains("checksum mismatch"), "{error}");
    assert!(
        !decode(&flipped)
            .expect("the salvage read takes it")
            .checksum_valid
    );

    // The second sector's data length, longer than everything after it.
    let damaged = resealed(|body| {
        body[SECOND_LEN_AT..SECOND_LEN_AT + 4].copy_from_slice(&64u32.to_le_bytes());
    });
    assert_eq!(
        decode(&damaged).map(|_| ()),
        Err(FormatError::Truncated("sector data"))
    );

    // A sector count past what the body holds.
    let counted = resealed(|body| {
        body[FIXED_HEADER_SIZE..V2_HEADER_SIZE].copy_from_slice(&u32::MAX.to_le_bytes());
    });
    assert!(
        matches!(
            decode(&counted),
            Err(FormatError::CountBeyondFile {
                declared: u32::MAX,
                ..
            })
        ),
        "{:?}",
        decode(&counted).map(|_| ())
    );
}

/// **A version-2 body too short for its own header is refused by the step**,
/// named as the migration that failed.
#[test]
fn a_v2_body_too_short_for_its_header_is_refused_by_the_migration() {
    let short = sealed(fixture_body()[..FIXED_HEADER_SIZE + 3].to_vec());
    assert_eq!(
        decode(&short).map(|_| ()),
        Err(FormatError::Migration {
            from: 2,
            to: 3,
            reason: "the body is shorter than a version-2 header",
        })
    );
    let storage = MemoryStorage::new();
    let path = Path::new("short.crb");
    storage
        .write(path, &short)
        .expect("a memory storage takes every write");
    assert!(matches!(
        SaveReader::open(&storage, path),
        Err(StorageError::Save(FormatError::Migration { from: 2, .. }))
    ));
}

/// **A save from a newer engine is refused by name**, never read as this
/// version's layout — whatever its checksum says.
#[test]
fn a_save_from_a_newer_engine_is_refused_by_name() {
    for version in [SAVE_FORMAT_VERSION + 1, u16::MAX] {
        assert_eq!(
            decode(&at_version(version)).map(|_| ()),
            Err(FormatError::Newer {
                found: version,
                current: SAVE_FORMAT_VERSION,
            })
        );
    }
}

/// **A version older than the chain reaches is refused by name**: version 1,
/// whose checksum nothing can verify, and a version 0 no writer wrote.
#[test]
fn a_version_older_than_the_chain_is_refused_by_name() {
    for version in [0, OLDEST_MIGRATABLE - 1] {
        assert_eq!(
            decode(&at_version(version)).map(|_| ()),
            Err(FormatError::Unmigratable {
                found: version,
                oldest: OLDEST_MIGRATABLE,
            })
        );
    }
}

/// **A step that does not write the version it migrates to is refused**, so
/// a body is never handed on to a reader for a layout it is not in.
#[test]
fn a_step_that_forgets_its_version_is_refused() {
    fn forgetful(body: &[u8]) -> Result<Vec<u8>, &'static str> {
        Ok(body.to_vec())
    }
    assert_eq!(
        run(&[forgetful], 2, fixture_body()).map(|_| ()),
        Err(FormatError::Migration {
            from: 2,
            to: 3,
            reason: "the step did not write the version it migrates to",
        })
    );
    assert!(run(&[v2_to_v3], 2, fixture_body()).is_ok());
}
