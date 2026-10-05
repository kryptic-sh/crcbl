//! The format, read back through the real container and a scratch
//! directory: what is kept, and every way a file is refused by name.
//!
//! The stage's half — that a resumed stage is the same run, tick for tick —
//! is `crate::game::checkpoint`'s tests, which can reach the stage.

use crcbl::store::save::{SAVE_FORMAT_VERSION, SaveData};

use super::payload::{PAYLOAD_VERSION, TOWER_BYTES};
use super::*;
use crate::tower::Kind::{Bolt, Slow, Splash};
use crate::wave::{self, STARTING_LIVES, WAVES};

/// A run resting after its second wave, with every value one a run between
/// waves can hold and the counters adding up: ten creeps released, six
/// killed, two leaked and two still walking, one of them held; three towers,
/// two of them stepped up; a bolt in the air at one creep and another whose
/// creep is gone, and a burst still drawn.
fn between_waves() -> Checkpoint {
    Checkpoint {
        map: Map::built_in().fingerprint(),
        runs: 4,
        ticks: 1_234,
        elapsed: 20.5,
        gold: 35,
        lives: STARTING_LIVES - 2,
        kills: 6,
        leaks: 2,
        shots: 23,
        built: 3,
        built_by_kind: [1, 1, 1],
        upgrades: 2,
        refused: 5,
        wave: 2,
        due_at: 21.75,
        towers: vec![
            SavedTower {
                plot: 0,
                kind: Bolt,
                tier: Tier::Upgraded,
                ready_at: 20.6,
                fired_at: 20.3,
            },
            SavedTower {
                plot: 2,
                kind: Splash,
                tier: Tier::Upgraded,
                ready_at: 19.9,
                fired_at: 19.2,
            },
            SavedTower {
                plot: 1,
                kind: Slow,
                tier: Tier::Base,
                ready_at: 0.0,
                fired_at: f64::NEG_INFINITY,
            },
        ],
        creeps: vec![
            SavedCreep {
                kind: crate::creep::Kind::Fast,
                along: 7.25,
                health: 3,
                slow: Slow.spec(Tier::Base).slow_factor,
            },
            SavedCreep {
                kind: crate::creep::Kind::Tanky,
                along: 1.5,
                health: crate::creep::Kind::Tanky.spec().health,
                slow: 1.0,
            },
        ],
        bolts: vec![
            SavedBolt {
                id: 21,
                at: DVec3::new(-5.5, 1.2, 2.0),
                heading: DVec3::new(0.6, -0.8, 0.0),
                target: Some(0),
                damage: Bolt.spec(Tier::Upgraded).damage,
                burst_m: Bolt.spec(Tier::Upgraded).burst_m,
            },
            SavedBolt {
                id: 22,
                at: DVec3::new(2.0, 0.9, 2.5),
                heading: DVec3::new(0.0, -1.0, 0.0),
                target: None,
                damage: Splash.spec(Tier::Upgraded).damage,
                burst_m: Splash.spec(Tier::Upgraded).burst_m,
            },
        ],
        bursts: vec![SavedBurst {
            id: 20,
            at: DVec3::new(-4.0, 0.0, 1.0),
            radius_m: Splash.spec(Tier::Upgraded).burst_m,
            raised_at: 20.45,
        }],
    }
}

/// A field that is not the committed one: a straight lane with a plot either
/// side.
fn another_map() -> Map {
    let plot = |label: &str, z: f64| crate::scene::Plot {
        label: label.to_string(),
        position: [0.0, 0.0, z],
    };
    Map::new(
        vec![
            crcbl::math::DVec3::new(-10.0, 0.0, 0.0),
            crcbl::math::DVec3::new(10.0, 0.0, 0.0),
        ],
        vec![plot("north", -3.0), plot("south", 3.0)],
    )
    .expect("a straight lane with a plot either side is a map")
}

/// `payload` as the one sector of a save written at `checkpoint`'s tick and
/// clock — what [`decode`] is handed once the container has opened.
fn saved(payload: Vec<u8>, checkpoint: &Checkpoint) -> SaveData {
    SaveData {
        header: SaveHeader::new(TickId::from_raw(checkpoint.ticks), checkpoint.elapsed),
        sectors: vec![SectorSave {
            sector_id: SectorId::ZERO,
            snapshot_data: payload,
        }],
        checksum_valid: true,
        format_version: SAVE_FORMAT_VERSION,
    }
}

/// [`decode`] of `checkpoint` as [`encode`] writes it, on the committed field.
fn round_trip(checkpoint: &Checkpoint) -> Result<Checkpoint, SaveError> {
    decode(&saved(encode(checkpoint), checkpoint), &Map::built_in())
}

/// A scratch directory, which every test that writes a file keeps its saves
/// in — never a real data directory.
fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().expect("a scratch directory")
}

/// **A run written to a directory is the run read back from it**, through
/// the real container — every field, the floats bit for bit.
#[test]
fn a_saved_run_comes_back_exactly_as_it_went_in() {
    let dir = scratch();
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    let map = Map::built_in();
    assert!(
        vault.load(&map).expect("no file is no error").is_none(),
        "nothing has been written yet",
    );
    vault
        .store(&between_waves())
        .expect("the scratch directory is writable");
    assert!(dir.path().join(PLAYER_FILE).is_file());

    let read = vault
        .load(&map)
        .expect("the save this build just wrote")
        .expect("a save is there");
    assert_eq!(read, between_waves());
    // `PartialEq` on a float is not bit equality — a NaN would differ and a
    // signed zero would not — and the resumed run is held to bits.
    for (read, wrote) in read.towers.iter().zip(&between_waves().towers) {
        assert_eq!(read.ready_at.to_bits(), wrote.ready_at.to_bits());
        assert_eq!(read.fired_at.to_bits(), wrote.fired_at.to_bits());
    }
    assert_eq!(read.elapsed.to_bits(), between_waves().elapsed.to_bits());
    assert_eq!(read.due_at.to_bits(), between_waves().due_at.to_bits());
}

/// **A file whose bytes were tampered with is refused by the container's
/// checksum**, named as a file that cannot be read — the half [`decode`]
/// cannot see, since a flipped count can still decode.
#[test]
fn a_corrupt_save_is_refused_by_its_checksum() {
    let dir = scratch();
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    vault
        .store(&between_waves())
        .expect("the scratch directory is writable");
    let file = dir.path().join(PLAYER_FILE);
    let mut bytes = std::fs::read(&file).expect("the save this test wrote");
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0xFF;
    std::fs::write(&file, &bytes).expect("the scratch directory is writable");

    let error = vault
        .load(&Map::built_in())
        .expect_err("a corrupt save was resumed");
    assert!(
        matches!(&error, SaveError::Unreadable(_)) && error.to_string().contains("checksum"),
        "{error}",
    );
}

/// A run saved by the version-2 container, before the engine version and the
/// scene reference were in its header: [`between_waves`] at payload version
/// 1, written by this module's `write` when the container was at version 2.
///
/// Its payload is this game's and its migration would be too: a payload bump
/// that cannot read it any more must migrate it or say why it is dropped.
const V2_RUN: &[u8] = include_bytes!("../../tests/fixtures/run-v2.crb");

/// **A run saved by an older container still resumes**, exactly as it went
/// in: `crcbl-store` migrates the container on open, and the payload inside it
/// is untouched.
#[test]
fn a_run_saved_by_the_version_2_container_still_resumes() {
    assert_eq!(
        V2_RUN[8..10],
        2u16.to_le_bytes(),
        "the fixture is a version-2 save"
    );
    let dir = scratch();
    std::fs::write(dir.path().join(PLAYER_FILE), V2_RUN)
        .expect("the scratch directory is writable");
    let read = Vault::at(dir.path().to_path_buf(), PLAYER_FILE)
        .load(&Map::built_in())
        .expect("a version-2 save resumes")
        .expect("a save is there");
    assert_eq!(read, between_waves());
    assert_eq!(read.elapsed.to_bits(), between_waves().elapsed.to_bits());
}

/// **A cut-short save is refused, wherever it was cut.** The file cut in half
/// is the container's to refuse; the payload cut at every length is
/// [`decode`]'s, and none of those prefixes reads as a run — or panics.
#[test]
fn a_truncated_save_is_refused_wherever_it_was_cut() {
    let dir = scratch();
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    vault
        .store(&between_waves())
        .expect("the scratch directory is writable");
    let file = dir.path().join(PLAYER_FILE);
    let bytes = std::fs::read(&file).expect("the save this test wrote");
    std::fs::write(&file, &bytes[..bytes.len() / 2]).expect("the scratch directory is writable");
    assert!(
        matches!(vault.load(&Map::built_in()), Err(SaveError::Unreadable(_))),
        "a file cut in half was not refused by the container",
    );

    let checkpoint = between_waves();
    let payload = encode(&checkpoint);
    assert!(round_trip(&checkpoint).is_ok(), "the control");
    for length in 0..payload.len() {
        let cut = saved(payload[..length].to_vec(), &checkpoint);
        assert!(
            matches!(decode(&cut, &Map::built_in()), Err(SaveError::Truncated)),
            "a payload cut to {length} of {} bytes was not refused as truncated",
            payload.len(),
        );
    }
    let mut longer = payload;
    longer.push(0);
    assert!(
        matches!(
            decode(&saved(longer, &checkpoint), &Map::built_in()),
            Err(SaveError::TrailingBytes { count: 1 })
        ),
        "a byte past the last tower was read anyway",
    );
}

/// **A save of another version is refused by name**, and the bytes are this
/// build's own with only the version stamped over — so the version check is
/// the only thing that can refuse them.
#[test]
fn a_save_of_another_version_is_refused_by_name() {
    let checkpoint = between_waves();
    let mut payload = encode(&checkpoint);
    assert!(round_trip(&checkpoint).is_ok(), "the control");
    for other in [0, PAYLOAD_VERSION + 1, u16::MAX] {
        payload[4..6].copy_from_slice(&other.to_le_bytes());
        let error = decode(&saved(payload.clone(), &checkpoint), &Map::built_in())
            .expect_err("another version was read");
        assert!(
            matches!(error, SaveError::Version { found } if found == other),
            "{error}",
        );
    }

    let mut foreign = encode(&checkpoint);
    foreign[0] = b'X';
    assert!(matches!(
        decode(&saved(foreign, &checkpoint), &Map::built_in()),
        Err(SaveError::NotTowers)
    ));
}

/// **A save played on another map is refused by name**, read against a map
/// whose plots it would otherwise fit: the fingerprint is the only thing
/// that refuses it.
#[test]
fn a_save_of_another_map_is_refused_by_name() {
    let mut checkpoint = between_waves();
    // Towers on the two plots the other map has, so nothing but the map's
    // identity stands between this payload and a resumed run on it.
    checkpoint.towers.truncate(2);
    checkpoint.towers[1].plot = 1;
    checkpoint.built = 2;
    checkpoint.built_by_kind = [1, 1, 0];
    let other = another_map();
    let payload = encode(&checkpoint);
    assert!(
        matches!(
            decode(&saved(payload.clone(), &checkpoint), &other),
            Err(SaveError::OtherMap)
        ),
        "a save of the committed field was resumed on another map",
    );
    let mut on_the_other = checkpoint.clone();
    on_the_other.map = other.fingerprint();
    assert!(
        decode(&saved(encode(&on_the_other), &on_the_other), &other).is_ok(),
        "the control: the same run, named for the other map, is read on it",
    );

    // …and from the vault, where `--resume` and the lobby read it.
    let dir = scratch();
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);
    vault
        .store(&checkpoint)
        .expect("the scratch directory is writable");
    assert!(matches!(vault.load(&other), Err(SaveError::OtherMap)));
}

/// A rule broken, named: one value of a sound save changed.
type Breaking = (&'static str, fn(&mut Checkpoint));

/// **Every rule a run between waves keeps is held on read**, each broken by
/// one value in an otherwise sound save — written by [`encode`], so this is
/// the reader's rule and not a byte offset.
#[test]
fn a_save_no_run_between_waves_could_write_is_refused_by_name() {
    assert!(round_trip(&between_waves()).is_ok(), "the control");

    let broken: Vec<Breaking> = vec![
        ("a NaN clock", |c| c.elapsed = f64::NAN),
        ("a clock before the start", |c| c.elapsed = -1.0),
        ("run zero", |c| c.runs = 0),
        ("a wave past the table", |c| c.wave = WAVES.len()),
        ("a wave due further off than a build phase", |c| {
            c.due_at = c.elapsed + wave::GAP_S + 1.0
        }),
        ("a wave due at NaN", |c| c.due_at = f64::NAN),
        ("lives with no leak to show", |c| c.lives += 1),
        ("no lives left", |c| {
            c.lives = 0;
            c.leaks = u64::from(STARTING_LIVES);
        }),
        ("kills the released creeps do not add up to", |c| {
            c.kills += 1
        }),
        ("more gold than the table pays", |c| c.gold = u32::MAX),
        ("built towers not standing", |c| c.built += 1),
        ("a kind's count not standing", |c| c.built_by_kind[0] += 1),
        ("upgrades not stepped up", |c| c.upgrades += 1),
        ("a tower on a plot the map lacks", |c| {
            c.towers[0].plot = Map::built_in().plots().len()
        }),
        ("two towers on one plot", |c| {
            c.towers[1].plot = c.towers[0].plot
        }),
        ("a tower ready after a reload from now", |c| {
            c.towers[0].ready_at = c.elapsed + 1.0
        }),
        ("a tower that fired after now", |c| {
            c.towers[0].fired_at = c.elapsed + 0.01
        }),
        ("a tower that fired at NaN", |c| {
            c.towers[0].fired_at = f64::NAN
        }),
        ("a creep off the path", |c| c.creeps[0].along = 1.0e6),
        ("a dead creep", |c| c.creeps[0].health = 0),
        ("a creep healthier than its kind", |c| {
            c.creeps[1].health += 1
        }),
        ("a hold no tower has", |c| c.creeps[0].slow = 0.42),
        ("a creep the waves never released", |c| {
            let walking = c.creeps[1];
            c.creeps.push(walking);
        }),
        ("a bolt at a creep not on the field", |c| {
            c.bolts[0].target = Some(2)
        }),
        ("a bolt no tower fires", |c| c.bolts[0].damage += 1),
        ("a bolt not yet fired", |c| c.bolts[0].id = c.shots),
        ("a bolt heading nowhere a shot heads", |c| {
            c.bolts[0].heading = DVec3::new(3.0, 0.0, 0.0)
        }),
        ("a bolt off the field", |c| {
            c.bolts[1].at = DVec3::new(f64::INFINITY, 0.0, 0.0)
        }),
        ("a burst no row raises", |c| c.bursts[0].radius_m = 1.0),
        ("a burst raised after now", |c| {
            c.bursts[0].raised_at = c.elapsed + 1.0
        }),
    ];
    for (what, breaking) in broken {
        let mut checkpoint = between_waves();
        breaking(&mut checkpoint);
        let error = round_trip(&checkpoint).expect_err(what);
        assert!(
            matches!(error, SaveError::Invalid(_)),
            "{what} was refused as something else: {error}",
        );
    }
}

/// **A byte no tower or kind table has is refused**, and the tower count is
/// held to the map's plots before anything is read for it — a count of
/// `u32::MAX` is a refusal, not a reservation.
#[test]
fn a_byte_this_build_never_wrote_is_refused_by_name() {
    let checkpoint = between_waves();
    let payload = encode(&checkpoint);
    // Where the module docs' table puts them: the kind count, then a count
    // per kind, the upgrades, the refusals, the wave and when the next is
    // due, then the tower count and the first tower.
    let kinds_at = 86;
    let count_at = kinds_at + 1 + 8 * tower::KINDS + 8 + 8 + 4 + 8;
    let towers_at = count_at + 4;
    assert!(payload.len() > towers_at + checkpoint.towers.len() * TOWER_BYTES);
    assert_eq!(
        payload[kinds_at],
        tower::KINDS as u8,
        "the kind count is where the module docs put it",
    );

    let refused = |payload: Vec<u8>| decode(&saved(payload, &checkpoint), &Map::built_in());

    let mut kinds = payload.clone();
    kinds[kinds_at] += 1;
    assert!(matches!(refused(kinds), Err(SaveError::Invalid(_))));

    let mut many = payload.clone();
    many[count_at..towers_at].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(refused(many), Err(SaveError::Invalid(_))));

    let mut kind = payload.clone();
    kind[towers_at + 1] = tower::KINDS as u8;
    assert!(matches!(refused(kind), Err(SaveError::Invalid(_))));

    let mut tier = payload.clone();
    tier[towers_at + 2] = tower::TIERS as u8;
    assert!(matches!(refused(tier), Err(SaveError::Invalid(_))));

    let mut two_sectors = saved(payload.clone(), &checkpoint);
    two_sectors.sectors.push(two_sectors.sectors[0].clone());
    assert!(matches!(
        decode(&two_sectors, &Map::built_in()),
        Err(SaveError::NotOneSector)
    ));

    // Every bit flipped, one at a time: refused, or read as exactly the run
    // the bytes say — never a panic, and never a run that writes back as
    // other bytes, which would be a field read wrong or not read at all.
    let (mut refusals, mut reads) = (0, 0);
    for at in 0..payload.len() {
        for bit in 0..8 {
            let mut flipped = payload.clone();
            flipped[at] ^= 1 << bit;
            match refused(flipped.clone()) {
                Err(_) => refusals += 1,
                Ok(read) => {
                    assert_eq!(
                        encode(&read),
                        flipped,
                        "bit {bit} of byte {at} flipped was read as another run",
                    );
                    reads += 1;
                }
            }
        }
    }
    // Both arms ran: a counter nothing else checks takes any value, and the
    // magic takes none but its own.
    assert!(
        refusals > 0 && reads > 0,
        "{refusals} refused, {reads} read"
    );
}

/// **A headless run keeps nothing and writes nothing**: the rule that lets
/// the test suite and CI run towers without touching a real data directory.
#[test]
fn a_headless_run_has_nowhere_to_save_and_finds_nothing() {
    let vault = Vault::player(true);
    assert_eq!(vault.where_it_goes(), "nowhere");
    assert!(matches!(vault.load(&Map::built_in()), Ok(None)));
    assert!(matches!(
        vault.store(&between_waves()),
        Err(SaveError::Nowhere)
    ));
}

/// The checkpoint a solo game on the committed field autosaves at its first
/// wave's end, asking for nothing — a real run's, for the tests elsewhere in
/// the crate that load one.
pub(crate) fn a_first_waves_end() -> Checkpoint {
    let mut game = crate::Game::new(crate::DEFAULT_TICK_HZ, &Map::built_in())
        .expect("a solo game always starts");
    // Far past the first build phase and the first wave's release.
    let most = 60 * u64::from(crate::DEFAULT_TICK_HZ);
    for _ in 0..most {
        game.tick();
        if let Some(checkpoint) = game.wave_end() {
            return checkpoint;
        }
    }
    panic!("the first wave did not end in {most} ticks");
}
