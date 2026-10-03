//! A spool read back: whole, cut short at every place a killed process can
//! cut it, damaged a bit at a time, and written by hand to break each rule.

use std::io::{self, Cursor, Read, Seek, SeekFrom};

use crcbl_net::ConsoleSet;

use super::*;
use crate::replay::{
    FileTransport, RecordedPeerFrames, RecordedRosterChange, ReplayStream, ReplayWriter,
    RosterChangeKind,
};

const TICK_RATE: u32 = 30;

/// One entry a recording makes, in the order it made them.
#[derive(Clone, Debug)]
enum Push {
    Set(u64, &'static str),
    Hash(u64, u64),
    Peer(u64, u64, Vec<u8>),
}

fn peer_tick(tick: u64, peer: u64, frame: &[u8]) -> RecordedPeerTick {
    RecordedPeerTick {
        tick: TickId::from_raw(tick),
        roster: vec![RecordedRosterChange {
            kind: RosterChangeKind::Joined,
            peer,
        }],
        peers: vec![RecordedPeerFrames {
            peer,
            dropped: 1,
            frames: vec![(TickId::from_raw(tick - 1), frame.to_vec())],
        }],
    }
}

fn spin_rate(value: &str) -> ConsoleSet {
    ConsoleSet {
        name: "sv_spin_rate".to_owned(),
        value: value.to_owned(),
    }
}

/// A session's entries, interleaved as a recorder pulls them: a hash every
/// tick, a peer tick most ticks, and a set now and then.
fn session() -> Vec<Push> {
    let mut pushes = vec![Push::Hash(1, 0x11)];
    for tick in 2..=8 {
        if tick % 3 == 0 {
            pushes.push(Push::Set(tick, if tick % 2 == 0 { "2.5" } else { "1" }));
        }
        if tick != 5 {
            pushes.push(Push::Peer(
                tick,
                tick % 2 + 1,
                vec![tick as u8; tick as usize],
            ));
        }
        pushes.push(Push::Hash(tick, tick * 0x101));
    }
    pushes
}

/// The spool a stream writes for `pushes`, and the file it finishes into.
fn spooled(pushes: &[Push]) -> (Vec<u8>, Vec<u8>) {
    let mut stream = ReplayStream::new(TICK_RATE, Cursor::new(Vec::new())).unwrap();
    for push in pushes {
        match push {
            Push::Set(tick, value) => stream
                .push_sim_set(TickId::from_raw(*tick), spin_rate(value))
                .unwrap(),
            Push::Hash(tick, hash) => stream
                .push_state_hash(TickId::from_raw(*tick), *hash)
                .unwrap(),
            Push::Peer(tick, peer, frame) => {
                stream
                    .push_peer_tick(&peer_tick(*tick, *peer, frame))
                    .unwrap();
            }
        }
    }
    let mut file = Vec::new();
    let spool = stream.finish(&mut file).unwrap().into_inner();
    (spool, file)
}

/// The file `ReplayWriter` writes for `pushes`: what a recovery keeping them
/// must write.
fn written(pushes: &[Push]) -> Vec<u8> {
    let mut writer = ReplayWriter::new(TICK_RATE);
    for push in pushes {
        match push {
            Push::Set(tick, value) => {
                writer.push_sim_set(TickId::from_raw(*tick), spin_rate(value))
            }
            Push::Hash(tick, hash) => writer.push_state_hash(TickId::from_raw(*tick), *hash),
            Push::Peer(tick, peer, frame) => writer.push_peer_tick(peer_tick(*tick, *peer, frame)),
        }
    }
    writer.encode().unwrap()
}

fn recover(spool: &[u8]) -> Result<(SpoolRecovery, Vec<u8>), StorageError> {
    let mut out = Vec::new();
    let recovery = recover_spool(Cursor::new(spool), &mut out)?;
    Ok((recovery, out))
}

/// Where each record of `spool` ends, read from the lengths in their heads.
fn record_ends(spool: &[u8]) -> Vec<usize> {
    let mut ends = Vec::new();
    let mut at = SPOOL_HEADER_BYTES;
    while at < spool.len() {
        let len = u32::from_le_bytes(spool[at + 1..at + 5].try_into().unwrap()) as usize;
        at += RECORD_HEAD_BYTES + len + RECORD_CRC_BYTES;
        ends.push(at);
    }
    assert_eq!(at, spool.len(), "the spool ends at a record's end");
    ends
}

/// **A whole spool recovers to the file its stream finishes into**, byte
/// for byte, dropping nothing — the file `ReplayWriter` writes for the same
/// entries.
#[test]
fn a_whole_spool_recovers_to_the_file_its_stream_finishes() {
    let pushes = session();
    let (spool, finished) = spooled(&pushes);
    assert_eq!(finished, written(&pushes));
    let (recovery, file) = recover(&spool).unwrap();
    assert_eq!(file, finished);
    assert_eq!(
        recovery,
        SpoolRecovery {
            tick_rate: TICK_RATE,
            sim_sets: 2,
            state_hashes: 8,
            hashed_ticks: Some((TickId::from_raw(1), TickId::from_raw(8))),
            peer_ticks: 6,
            kept_bytes: spool.len() as u64,
            dropped_bytes: 0,
            end: SpoolEnd::Whole,
        }
    );

    // The spool of a stream that recorded nothing is its header, and
    // recovers to the empty file.
    let (empty, _) = spooled(&[]);
    assert_eq!(empty, encode_header(TICK_RATE));
    assert_eq!(recover(&empty).unwrap().1, written(&[]));
}

/// **A spool cut short keeps every whole record and reports the rest**: cut
/// at every byte of every record — what a process killed while writing that
/// record leaves — the file holds the records before it, as `ReplayWriter`
/// writes them, and reads back.
#[test]
fn a_spool_cut_inside_a_record_keeps_every_whole_one_before_it() {
    let pushes = session();
    let (spool, _) = spooled(&pushes);
    let ends = record_ends(&spool);
    assert_eq!(ends.len(), pushes.len());
    let mut start = SPOOL_HEADER_BYTES;
    for (record, &end) in ends.iter().enumerate() {
        for cut in start + 1..end {
            let (recovery, file) = recover(&spool[..cut]).unwrap();
            assert_eq!(recovery.end, SpoolEnd::CutShort, "cut at {cut}");
            assert_eq!(recovery.kept_bytes, start as u64, "cut at {cut}");
            assert_eq!(recovery.dropped_bytes, (cut - start) as u64, "cut at {cut}");
            assert_eq!(file, written(&pushes[..record]), "cut at {cut}");
            FileTransport::decode(&file).expect("the recovered file reads");
        }
        // Cut at the record's end, nothing is dropped.
        let (recovery, file) = recover(&spool[..end]).unwrap();
        assert_eq!(recovery.end, SpoolEnd::Whole);
        assert_eq!(file, written(&pushes[..=record]));
        start = end;
    }
}

/// **A spool cut inside its header is refused by name**: it has no tick
/// rate to write a file with.
#[test]
fn a_spool_without_a_whole_header_is_refused() {
    let (spool, _) = spooled(&session());
    for cut in 0..SPOOL_HEADER_BYTES {
        match recover(&spool[..cut]) {
            Err(StorageError::ReplaySpool(SpoolError::TooShort(len))) => {
                assert_eq!(len, cut as u64);
            }
            other => panic!("cut at {cut}: {other:?}"),
        }
    }
    let refused = |header: &[u8]| match recover(header) {
        Err(StorageError::ReplaySpool(error)) => error,
        other => panic!("not refused: {other:?}"),
    };
    let mut magic = spool.clone();
    magic[0] = b'X';
    assert_eq!(refused(&magic), SpoolError::Magic);
    let mut version = spool.clone();
    version[8..10].copy_from_slice(&(SPOOL_FORMAT_VERSION + 1).to_le_bytes());
    assert_eq!(
        refused(&version),
        SpoolError::Version(SPOOL_FORMAT_VERSION + 1)
    );
    let mut rate = spool;
    rate[10] ^= 1;
    assert_eq!(refused(&rate), SpoolError::HeaderChecksum);
}

/// **A bit flipped anywhere in a record stops the recovery at that record**,
/// never past it and never in a panic: every bit of every record is flipped
/// in turn, and the file holds exactly the records before the damaged one.
#[test]
fn a_flipped_bit_stops_the_recovery_at_its_record() {
    let pushes = session();
    let (spool, _) = spooled(&pushes);
    let ends = record_ends(&spool);
    let mut start = SPOOL_HEADER_BYTES;
    for (record, &end) in ends.iter().enumerate() {
        let expected = written(&pushes[..record]);
        for byte in start..end {
            for bit in 0..8 {
                let mut damaged = spool.clone();
                damaged[byte] ^= 1 << bit;
                let (recovery, file) = recover(&damaged).unwrap();
                assert_ne!(recovery.end, SpoolEnd::Whole, "byte {byte} bit {bit}");
                assert_eq!(recovery.kept_bytes, start as u64, "byte {byte} bit {bit}");
                assert_eq!(file, expected, "byte {byte} bit {bit}");
            }
        }
        start = end;
    }
}

/// Bytes after a whole header that no stream wrote: a length past the end,
/// a kind no build writes, and noise — each stops the recovery at once,
/// reserving nothing for the length.
#[test]
fn garbage_after_the_header_keeps_nothing() {
    let header = encode_header(TICK_RATE);
    let with = |tail: &[u8]| {
        let mut spool = header.to_vec();
        spool.extend_from_slice(tail);
        recover(&spool).unwrap()
    };
    let mut huge = vec![RecordKind::PeerTick.code()];
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    huge.extend_from_slice(&[0; 16]);
    let (recovery, file) = with(&huge);
    assert_eq!(recovery.end, SpoolEnd::CutShort);
    assert_eq!(recovery.dropped_bytes, huge.len() as u64);
    assert_eq!(file, written(&[]));

    let (recovery, _) = with(&[9, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(recovery.end, SpoolEnd::UnknownRecord(9));

    // A fixed noise pattern, every byte value, at every length.
    let noise: Vec<u8> = (0..=255u8).map(|n| n.wrapping_mul(167) ^ 0x5A).collect();
    for len in 0..noise.len() {
        let (recovery, file) = with(&noise[..len]);
        assert_eq!(recovery.kept_bytes, SPOOL_HEADER_BYTES as u64);
        assert_eq!(file, written(&[]));
    }
}

/// `payload` framed as a record of `kind` whose CRC matches: a record a
/// stream never writes, since it breaks a rule.
fn framed(kind: RecordKind, payload: &[u8]) -> Vec<u8> {
    let mut record = Vec::new();
    frame(kind, &mut record, |buf| {
        buf.extend_from_slice(payload);
        Ok(())
    })
    .unwrap();
    record
}

fn hash_payload(tick: u64, hash: u64) -> Vec<u8> {
    let mut payload = Vec::new();
    input::encode_state_hash(
        &RecordedStateHash {
            tick: TickId::from_raw(tick),
            hash,
        },
        &mut payload,
    );
    payload
}

/// **A whole record that breaks a rule stops the recovery there**, by the
/// rule: a hash for a tick already hashed, a payload with bytes its entry
/// does not read, a kind no build writes — each with a CRC that matches.
#[test]
fn a_whole_record_breaking_a_rule_stops_the_recovery_there() {
    let mut spool = encode_header(TICK_RATE).to_vec();
    spool.extend_from_slice(&framed(RecordKind::StateHash, &hash_payload(3, 7)));
    let kept = spool.len() as u64;
    let with = |record: Vec<u8>| {
        let mut damaged = spool.clone();
        damaged.extend_from_slice(&record);
        damaged.extend_from_slice(&framed(RecordKind::StateHash, &hash_payload(9, 9)));
        recover(&damaged).unwrap()
    };

    let (recovery, file) = with(framed(RecordKind::StateHash, &hash_payload(3, 8)));
    assert_eq!(
        recovery.end,
        SpoolEnd::Refused(InputSectionError::HashOutOfOrder {
            previous: TickId::from_raw(3),
            tick: TickId::from_raw(3),
        })
    );
    assert_eq!(recovery.kept_bytes, kept);
    assert_eq!(file, written(&[Push::Hash(3, 7)]));

    let mut long = hash_payload(4, 8);
    long.push(0);
    let (recovery, _) = with(framed(RecordKind::StateHash, &long));
    assert_eq!(
        recovery.end,
        SpoolEnd::Refused(InputSectionError::TrailingBytes(1))
    );
    assert_eq!(recovery.kept_bytes, kept);

    let (recovery, _) = with(framed(RecordKind::PeerTick, &hash_payload(4, 8)));
    assert!(
        matches!(
            recovery.end,
            SpoolEnd::Refused(InputSectionError::CountBeyondFile { .. })
        ),
        "{:?}",
        recovery.end
    );
    assert_eq!(recovery.kept_bytes, kept);

    let mut unknown = framed(RecordKind::StateHash, &hash_payload(4, 8));
    unknown[0] = 0;
    let (recovery, _) = with(unknown);
    assert_eq!(recovery.end, SpoolEnd::UnknownRecord(0));
}

/// A spool that reads as one set of bytes until it is opened a second time,
/// and as another after: something writing to it between the scan and the
/// copy.
#[derive(Debug)]
struct Rewritten {
    before: Cursor<Vec<u8>>,
    after: Cursor<Vec<u8>>,
    opened: u32,
}

impl Rewritten {
    fn current(&mut self) -> &mut Cursor<Vec<u8>> {
        if self.opened > 1 {
            &mut self.after
        } else {
            &mut self.before
        }
    }
}

impl Read for Rewritten {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.current().read(buf)
    }
}

impl Seek for Rewritten {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        if pos == SeekFrom::Start(0) {
            self.opened += 1;
        }
        self.current().seek(pos)
    }
}

/// **A spool that changes between the scan and the copy is refused**, not
/// copied into a file that counts records it does not hold.
#[test]
fn a_spool_changing_while_it_is_read_is_refused() {
    let (spool, _) = spooled(&session());
    let mut damaged = spool.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    let rewritten = Rewritten {
        before: Cursor::new(spool),
        after: Cursor::new(damaged),
        opened: 0,
    };
    match recover_spool(rewritten, &mut Vec::new()) {
        Err(StorageError::ReplaySpool(SpoolError::Changed)) => {}
        other => panic!("not refused as changed: {other:?}"),
    }
}
