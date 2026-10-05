//! `crcbl save diff`: what differs between two saves, field by field and
//! sector by sector.

use std::collections::BTreeMap;
use std::path::Path;

use crcbl_store::save::{SaveData, SaveHeader, SectorSave};

use super::{engine, open, playtime, refused, scene};
use crate::json::Json;
use crate::report::{EXIT_DIFFERENT, EXIT_OK, EXIT_TROUBLE, Failure, Outcome};

/// `crcbl save diff`, with the code its answer exits with.
pub(super) fn run(a: &Path, b: &Path) -> (Result<Outcome, Failure>, u8) {
    let open_or_trouble = |file: &Path| {
        open(file).map_err(|error| Failure {
            code: EXIT_TROUBLE,
            ..refused(file, &error)
        })
    };
    let (left, right) = match (open_or_trouble(a), open_or_trouble(b)) {
        (Ok(left), Ok(right)) => (left, right),
        (Err(failure), _) | (_, Err(failure)) => return (Err(failure), EXIT_TROUBLE),
    };

    let header = header_differences(&left, &right);
    let sectors = sector_differences(&left.sectors, &right.sectors);
    let identical = header.is_empty() && sectors.is_empty();
    let (a_name, b_name) = (a.display().to_string(), b.display().to_string());

    let mut lines = Vec::new();
    for field in &header {
        lines.push(format!(
            "  header {}: {} | {}",
            field.name, field.a.1, field.b.1
        ));
    }
    for sector in &sectors {
        let name = sector.name();
        lines.push(match (sector.a, sector.b) {
            (Some(len), None) => format!("  sector {name}: only in {a_name}, {len} bytes"),
            (None, Some(len)) => format!("  sector {name}: only in {b_name}, {len} bytes"),
            (a_len, b_len) => format!(
                "  sector {name}: bytes differ from offset {}; lengths {} and {}",
                sector.first_difference.unwrap_or_default(),
                a_len.unwrap_or_default(),
                b_len.unwrap_or_default()
            ),
        });
    }
    let human = if identical {
        format!("{a_name} and {b_name} are the same: every header field and every sector")
    } else {
        format!("{a_name} and {b_name} differ\n{}", lines.join("\n"))
    };

    let only = |in_a: bool| {
        Json::Array(
            sectors
                .iter()
                .filter_map(|sector| match (sector.a, sector.b) {
                    (Some(len), None) if in_a => Some(sector.json_one(len)),
                    (None, Some(len)) if !in_a => Some(sector.json_one(len)),
                    _ => None,
                })
                .collect(),
        )
    };
    let changed = Json::Array(
        sectors
            .iter()
            .filter_map(|sector| match (sector.a, sector.b) {
                (Some(a_len), Some(b_len)) => Some(Json::Object(vec![
                    ("sector", Json::Array(sector.key.coordinates())),
                    ("occurrence", Json::Unsigned(sector.key.occurrence as u64)),
                    ("length_a", Json::Unsigned(a_len as u64)),
                    ("length_b", Json::Unsigned(b_len as u64)),
                    (
                        "first_difference",
                        Json::Unsigned(sector.first_difference.unwrap_or_default() as u64),
                    ),
                ])),
                _ => None,
            })
            .collect(),
    );
    let json = vec![
        ("a", Json::string(a_name)),
        ("b", Json::string(b_name)),
        ("identical", Json::Bool(identical)),
        (
            "header",
            Json::Array(
                header
                    .into_iter()
                    .map(|field| {
                        Json::Object(vec![
                            ("field", Json::string(field.name)),
                            ("a", field.a.0),
                            ("b", field.b.0),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("only_in_a", only(true)),
        ("only_in_b", only(false)),
        ("changed", changed),
    ];
    let code = if identical { EXIT_OK } else { EXIT_DIFFERENT };
    (Ok(Outcome { human, json }), code)
}

/// One header field that differs, as JSON and as text on each side.
struct FieldDifference {
    name: &'static str,
    a: (Json, String),
    b: (Json, String),
}

/// Every header field of `a` and `b` that differs, in the order a header is
/// written. The playtime is compared by its bits, so a field is the same
/// exactly when its bytes in the file are.
fn header_differences(a: &SaveData, b: &SaveData) -> Vec<FieldDifference> {
    let (ha, hb) = (&a.header, &b.header);
    let version = |data: &SaveData| {
        (
            Json::Number(data.format_version.into()),
            data.format_version.to_string(),
        )
    };
    let tick = |header: &SaveHeader| {
        (
            Json::Unsigned(header.tick.get()),
            header.tick.get().to_string(),
        )
    };
    let fields = [
        (
            "format_version",
            a.format_version == b.format_version,
            version(a),
            version(b),
        ),
        ("tick", ha.tick == hb.tick, tick(ha), tick(hb)),
        (
            "playtime_secs",
            ha.playtime_secs.to_bits() == hb.playtime_secs.to_bits(),
            playtime(ha),
            playtime(hb),
        ),
        (
            "engine_version",
            ha.engine_version == hb.engine_version,
            engine(ha),
            engine(hb),
        ),
        ("scene", ha.scene == hb.scene, scene(ha), scene(hb)),
    ];
    fields
        .into_iter()
        .filter(|(_, same, _, _)| !same)
        .map(|(name, _, a, b)| FieldDifference { name, a, b })
        .collect()
}

/// A sector's place in a file: its coordinates, and which of the sectors at
/// those coordinates it is, counting from 1. The container does not forbid two
/// sectors at one place, so the second is matched to the other file's second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SectorKey {
    x: i64,
    y: i64,
    z: i64,
    occurrence: usize,
}

impl SectorKey {
    fn coordinates(self) -> Vec<Json> {
        vec![
            Json::Number(self.x),
            Json::Number(self.y),
            Json::Number(self.z),
        ]
    }
}

/// A sector that differs: its length on each side it is on, and where its
/// bytes first differ when it is on both.
struct SectorDifference {
    key: SectorKey,
    a: Option<usize>,
    b: Option<usize>,
    first_difference: Option<usize>,
}

impl SectorDifference {
    fn name(&self) -> String {
        let key = self.key;
        let place = format!("({}, {}, {})", key.x, key.y, key.z);
        if key.occurrence > 1 {
            format!("{place} (occurrence {})", key.occurrence)
        } else {
            place
        }
    }

    /// The record for a sector only one file has.
    fn json_one(&self, length: usize) -> Json {
        Json::Object(vec![
            ("sector", Json::Array(self.key.coordinates())),
            ("occurrence", Json::Unsigned(self.key.occurrence as u64)),
            ("length", Json::Unsigned(length as u64)),
        ])
    }
}

/// `sectors` by their place, so two files are compared sector for sector
/// whatever order each lists them in.
fn by_place(sectors: &[SectorSave]) -> BTreeMap<SectorKey, &[u8]> {
    let mut seen: BTreeMap<(i64, i64, i64), usize> = BTreeMap::new();
    let mut keyed = BTreeMap::new();
    for sector in sectors {
        let id = sector.sector_id;
        let count = seen.entry((id.x, id.y, id.z)).or_default();
        *count += 1;
        let key = SectorKey {
            x: id.x,
            y: id.y,
            z: id.z,
            occurrence: *count,
        };
        keyed.insert(key, sector.snapshot_data.as_slice());
    }
    keyed
}

/// Every sector only one of `a` and `b` has, and every one whose bytes
/// differ, in coordinate order.
fn sector_differences(a: &[SectorSave], b: &[SectorSave]) -> Vec<SectorDifference> {
    let (a, b) = (by_place(a), by_place(b));
    let mut keys: Vec<SectorKey> = a.keys().chain(b.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter_map(|key| {
            let (left, right) = (a.get(&key).copied(), b.get(&key).copied());
            let first_difference = match (left, right) {
                (Some(left), Some(right)) => Some(first_difference(left, right)?),
                _ => None,
            };
            Some(SectorDifference {
                key,
                a: left.map(<[u8]>::len),
                b: right.map(<[u8]>::len),
                first_difference,
            })
        })
        .collect()
}

/// The first offset at which `a` and `b` differ — the shorter one's length
/// when it is a prefix of the other — or `None` when they are the same bytes.
fn first_difference(a: &[u8], b: &[u8]) -> Option<usize> {
    a.iter()
        .zip(b)
        .position(|(left, right)| left != right)
        .or_else(|| (a.len() != b.len()).then(|| a.len().min(b.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The first differing offset**, at the start, in the middle, past the
    /// end of a prefix, and nowhere.
    #[test]
    fn the_first_difference_is_the_first_offset_that_differs() {
        assert_eq!(first_difference(&[1, 2, 3], &[9, 2, 3]), Some(0));
        assert_eq!(first_difference(&[1, 2, 3], &[1, 2, 9]), Some(2));
        assert_eq!(first_difference(&[1, 2], &[1, 2, 3]), Some(2));
        assert_eq!(first_difference(&[1, 2, 3], &[1]), Some(1));
        assert_eq!(first_difference(&[1, 2, 3], &[1, 2, 3]), None);
        assert_eq!(first_difference(&[], &[]), None);
    }
}
