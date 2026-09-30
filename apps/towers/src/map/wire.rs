//! A map on the wire: what a LAN host sends each joiner at join, and what the
//! joiner builds its game on.
//!
//! **The host's map is the session's.** A joiner's own `--scene` plays no
//! part in a session it joins: the host sends its [`Map`] the moment the
//! joiner is admitted — sealed, on the reliable channel, ahead of anything
//! the joiner needs it for (`crate::lan`) — and the joiner builds nothing
//! until it has read it back through [`Map::from_wire`].
//!
//! # A defined encoding, never the memory
//!
//! Every value goes through the byte layout below, field by field — never
//! the bytes a [`Map`] happens to occupy, which would carry a `Vec`'s pointer
//! and capacity. Positions go in as [`f64::to_bits`], so a coordinate arrives
//! as exactly the value the host holds; labels as their UTF-8 bytes after
//! their length, so the stream says where each stops.
//!
//! ```text
//! WIRE_TAG
//! u32 LE  waypoint count
//!   per waypoint, in walk order:  x, y, z   (f64::to_bits, u64 LE)
//! u32 LE  plot count
//!   per plot, in list order:      u32 LE label length, label UTF-8,
//!                                 x, y, z   (f64::to_bits, u64 LE)
//! ```
//!
//! The order of both lists is part of the map: the path is walked in waypoint
//! order and `PlaceTower` numbers plots by position in theirs.
//!
//! # The bytes are a stranger's
//!
//! A joiner reads what arrived on a socket, so [`Map::from_wire`] trusts
//! nothing in it: every count and length is held to its cap —
//! [`MAX_WAYPOINTS`], [`MAX_PLOTS`], [`MAX_LABEL_BYTES`] — before anything is
//! allocated for it, every read is bounds-checked, a coordinate that is not
//! finite is refused (the rules below compare, and a NaN compares false
//! against every limit), and what is left is held to every rule a scene file
//! is, by [`Map::new`]. Each refusal is a [`MapWireError`] naming what was
//! wrong.
//!
//! # How big it gets
//!
//! The caps bound it: the largest map they admit is a couple of datagrams on
//! the reliable channel, and far inside the one field a server event may
//! carry — `the_largest_map_the_caps_admit_fits_one_event` builds that map
//! and measures it.

use crcbl::math::DVec3;

use super::{MAX_LABEL_BYTES, MAX_PLOTS, MAX_WAYPOINTS, Map, MapError};
use crate::scene::Plot;

/// What every encoding starts with, and what changes when the layout does: a
/// joiner of a build that encodes differently refuses the map as
/// [`MapWireError::NotAMap`] rather than reading one layout as the other.
const WIRE_TAG: &[u8] = b"crcbl towers map v1";

/// One coordinate's width on the wire.
const COORDINATE_BYTES: usize = size_of::<u64>();

/// One count's or length's width on the wire.
const COUNT_BYTES: usize = size_of::<u32>();

/// Why the bytes a host sent are not a map this build plays on.
#[derive(Debug)]
pub enum MapWireError {
    /// They do not start with this build's tag: another layout, or not a map
    /// at all.
    NotAMap,
    /// They stop partway through a field.
    Truncated {
        /// Where the field that ran out starts, in bytes.
        offset: usize,
        /// How many bytes it needed there.
        needed: usize,
    },
    /// A plot's label is not UTF-8.
    LabelNotUtf8 {
        /// Which plot, counted from the first.
        plot: usize,
    },
    /// A coordinate is infinite or not a number.
    NotFinite {
        /// Which point: `waypoint 2`, `plot 0`.
        what: String,
    },
    /// Bytes follow the last plot.
    TrailingBytes {
        /// How many.
        count: usize,
    },
    /// The layout breaks a rule every map is held to — a count or a label
    /// past its cap, a plot on the lane — named as [`Map::new`] names it.
    Map(MapError),
}

impl std::fmt::Display for MapWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAMap => write!(f, "the bytes are not a towers map this build reads"),
            Self::Truncated { offset, needed } => write!(
                f,
                "the map stops at byte {offset}, where {needed} more were needed"
            ),
            Self::LabelNotUtf8 { plot } => write!(f, "plot {plot}'s label is not UTF-8"),
            Self::NotFinite { what } => write!(f, "{what} has a coordinate that is not finite"),
            Self::TrailingBytes { count } => {
                write!(f, "{count} byte(s) follow the map's last plot")
            }
            Self::Map(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MapWireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Map(error) => Some(error),
            _ => None,
        }
    }
}

impl Map {
    /// This map in the wire layout the module docs give — what a host sends
    /// every joiner.
    #[must_use]
    pub fn to_wire(&self) -> Vec<u8> {
        let waypoints = self.path().waypoints();
        let mut bytes = WIRE_TAG.to_vec();
        push_count(&mut bytes, waypoints.len());
        for waypoint in waypoints {
            push_point(&mut bytes, *waypoint);
        }
        push_count(&mut bytes, self.plots().len());
        for plot in self.plots() {
            push_count(&mut bytes, plot.label.len());
            bytes.extend_from_slice(plot.label.as_bytes());
            push_point(&mut bytes, plot.at());
        }
        bytes
    }

    /// The map `bytes` carry, read as a stranger's — see the module docs.
    ///
    /// # Errors
    ///
    /// [`MapWireError`], naming what is wrong with them.
    pub fn from_wire(bytes: &[u8]) -> Result<Self, MapWireError> {
        let mut reader = Reader { bytes, offset: 0 };
        if reader.take(WIRE_TAG.len())? != WIRE_TAG {
            return Err(MapWireError::NotAMap);
        }

        let found = reader.count()?;
        if found > MAX_WAYPOINTS {
            return Err(MapWireError::Map(MapError::TooManyWaypoints { found }));
        }
        let mut waypoints = Vec::with_capacity(found);
        for index in 0..found {
            waypoints.push(reader.point(|| format!("waypoint {index}"))?);
        }

        let found = reader.count()?;
        if found > MAX_PLOTS {
            return Err(MapWireError::Map(MapError::TooManyPlots { found }));
        }
        let mut plots = Vec::with_capacity(found);
        for index in 0..found {
            let length = reader.count()?;
            if length > MAX_LABEL_BYTES {
                return Err(MapWireError::Map(MapError::LabelTooLong {
                    plot: index,
                    length,
                }));
            }
            let label = std::str::from_utf8(reader.take(length)?)
                .map_err(|_| MapWireError::LabelNotUtf8 { plot: index })?
                .to_string();
            let at = reader.point(|| format!("plot {index}"))?;
            plots.push(Plot {
                label,
                position: at.to_array(),
            });
        }

        let count = bytes.len() - reader.offset;
        if count != 0 {
            return Err(MapWireError::TrailingBytes { count });
        }
        Self::new(waypoints, plots).map_err(MapWireError::Map)
    }
}

/// A length as a little-endian `u32`. Every list and label here is under its
/// cap — [`Map::new`] holds them to it — so one past `u32` is a map nothing
/// could have built.
fn push_count(bytes: &mut Vec<u8>, count: usize) {
    let count = u32::try_from(count).expect("a map's lists and labels are under their caps");
    bytes.extend_from_slice(&count.to_le_bytes());
}

/// A point as its three coordinates' bits, little-endian.
fn push_point(bytes: &mut Vec<u8>, point: DVec3) {
    for value in point.to_array() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
}

/// A cursor over a wire map that bounds-checks every read.
struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    /// The next `needed` bytes.
    fn take(&mut self, needed: usize) -> Result<&'a [u8], MapWireError> {
        let end = self.offset.checked_add(needed);
        let Some(taken) = end.and_then(|end| self.bytes.get(self.offset..end)) else {
            return Err(MapWireError::Truncated {
                offset: self.offset,
                needed,
            });
        };
        self.offset += needed;
        Ok(taken)
    }

    /// The next count or length.
    fn count(&mut self) -> Result<usize, MapWireError> {
        let mut raw = [0; COUNT_BYTES];
        raw.copy_from_slice(self.take(COUNT_BYTES)?);
        // A `u32` fits every `usize` this crate builds for; one that did not
        // would be past every cap it is compared with anyway.
        Ok(usize::try_from(u32::from_le_bytes(raw)).unwrap_or(usize::MAX))
    }

    /// The next point, refused if any coordinate is not finite. `what` names
    /// it for the refusal.
    fn point(&mut self, what: impl Fn() -> String) -> Result<DVec3, MapWireError> {
        let mut coordinates = [0.0; 3];
        for coordinate in &mut coordinates {
            let mut raw = [0; COORDINATE_BYTES];
            raw.copy_from_slice(self.take(COORDINATE_BYTES)?);
            *coordinate = f64::from_bits(u64::from_le_bytes(raw));
            if !coordinate.is_finite() {
                return Err(MapWireError::NotFinite { what: what() });
            }
        }
        Ok(DVec3::from_array(coordinates))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One point's width on the wire: three coordinates.
    const POINT_BYTES: usize = 3 * COORDINATE_BYTES;

    /// The longest encoding any map [`Map::new`] accepts can have: the tag,
    /// the most waypoints, and the most plots each with the longest label.
    const MAX_WIRE_BYTES: usize = WIRE_TAG.len()
        + COUNT_BYTES
        + MAX_WAYPOINTS * POINT_BYTES
        + COUNT_BYTES
        + MAX_PLOTS * (COUNT_BYTES + MAX_LABEL_BYTES + POINT_BYTES);

    /// The committed field's waypoints and plots, taken apart so a test can
    /// change one thing and encode the map again.
    fn parts() -> (Vec<DVec3>, Vec<Plot>) {
        let map = Map::built_in();
        (map.path().waypoints().to_vec(), map.plots().to_vec())
    }

    /// The committed field's encoding with `edit` applied to its bytes.
    fn edited(edit: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut bytes = Map::built_in().to_wire();
        edit(&mut bytes);
        bytes
    }

    /// Where the first plot's record starts in the committed field's
    /// encoding: its label's length.
    fn first_plot_at() -> usize {
        WIRE_TAG.len()
            + COUNT_BYTES
            + Map::built_in().path().waypoints().len() * POINT_BYTES
            + COUNT_BYTES
    }

    /// **A map read back off the wire is the map sent**, bit for bit — the
    /// committed field, and a map with another shape entirely.
    #[test]
    fn a_map_survives_the_wire() {
        let field = Map::built_in();
        assert_eq!(Map::from_wire(&field.to_wire()).expect("decodes"), field);

        let (mut waypoints, mut plots) = parts();
        waypoints.truncate(2);
        plots.truncate(1);
        plots[0].label = "the only plot".into();
        let small = Map::new(waypoints, plots).expect("a legal map");
        assert_ne!(small, field);
        assert_eq!(Map::from_wire(&small.to_wire()).expect("decodes"), small);
    }

    /// **Every byte of it is read**: moving a plot, renaming one, or
    /// reordering either list is a different map at the other end.
    #[test]
    fn every_field_crosses_the_wire() {
        let (waypoints, plots) = parts();
        let mut moved = plots.clone();
        moved[2].position[0] += 0.25;
        let mut renamed = plots.clone();
        renamed[0].label = renamed[0].label.to_uppercase();
        let mut reordered = plots;
        reordered.swap(0, 1);
        for plots in [moved, renamed, reordered] {
            let map = Map::new(waypoints.clone(), plots).expect("still legal");
            let back = Map::from_wire(&map.to_wire()).expect("decodes");
            assert_eq!(back, map);
            assert_ne!(back, Map::built_in());
        }
    }

    /// **Bytes that are not this layout are refused by name**: another tag,
    /// a stream cut short anywhere, and bytes after the last plot.
    #[test]
    fn bytes_that_are_not_a_map_are_refused_by_name() {
        assert!(matches!(
            Map::from_wire(&edited(|bytes| bytes[0] ^= 1)),
            Err(MapWireError::NotAMap)
        ));
        assert!(matches!(
            Map::from_wire(b"crcbl"),
            Err(MapWireError::Truncated { offset: 0, .. })
        ));
        let whole = Map::built_in().to_wire();
        for cut in [
            WIRE_TAG.len(),
            WIRE_TAG.len() + 1,
            first_plot_at() + 2,
            whole.len() - 1,
        ] {
            assert!(
                matches!(
                    Map::from_wire(&whole[..cut]),
                    Err(MapWireError::Truncated { .. })
                ),
                "cut at {cut} of {}",
                whole.len()
            );
        }
        assert!(matches!(
            Map::from_wire(&edited(|bytes| bytes.push(0))),
            Err(MapWireError::TrailingBytes { count: 1 })
        ));
    }

    /// **A count or a length past its cap is refused before anything is
    /// allocated for it** — a count of four billion arrives as the rule it
    /// breaks, not as an allocation — and so is a label that is not UTF-8.
    #[test]
    fn a_count_or_label_past_its_cap_is_refused_before_it_is_read() {
        let huge = u32::MAX.to_le_bytes();
        let waypoints_at = WIRE_TAG.len();
        assert!(matches!(
            Map::from_wire(&edited(|bytes| {
                bytes[waypoints_at..waypoints_at + COUNT_BYTES].copy_from_slice(&huge);
            })),
            Err(MapWireError::Map(MapError::TooManyWaypoints { found })) if found == u32::MAX as usize
        ));
        let plots_at = first_plot_at() - COUNT_BYTES;
        assert!(matches!(
            Map::from_wire(&edited(|bytes| {
                bytes[plots_at..plots_at + COUNT_BYTES].copy_from_slice(&huge);
            })),
            Err(MapWireError::Map(MapError::TooManyPlots { .. }))
        ));
        let label_at = first_plot_at();
        assert!(matches!(
            Map::from_wire(&edited(|bytes| {
                bytes[label_at..label_at + COUNT_BYTES].copy_from_slice(&huge);
            })),
            Err(MapWireError::Map(MapError::LabelTooLong { plot: 0, .. }))
        ));
        assert!(matches!(
            Map::from_wire(&edited(|bytes| bytes[label_at + COUNT_BYTES] = 0xFF)),
            Err(MapWireError::LabelNotUtf8 { plot: 0 })
        ));
    }

    /// **A coordinate that is not finite is refused**, naming the point: a
    /// NaN compares false against every limit [`Map::new`] measures, so the
    /// rules alone would let one through.
    #[test]
    fn a_coordinate_that_is_not_finite_is_refused() {
        let waypoint_x = WIRE_TAG.len() + COUNT_BYTES + POINT_BYTES;
        for bad in [f64::NAN, f64::INFINITY] {
            assert!(matches!(
                Map::from_wire(&edited(|bytes| {
                    bytes[waypoint_x..waypoint_x + COORDINATE_BYTES]
                        .copy_from_slice(&bad.to_bits().to_le_bytes());
                })),
                Err(MapWireError::NotFinite { what }) if what == "waypoint 1"
            ));
        }
    }

    /// **A well-formed map that breaks a rule is refused as that rule** — a
    /// plot moved onto the lane, which the host could never have loaded.
    #[test]
    fn a_well_formed_map_is_still_held_to_the_rules() {
        let (waypoints, _) = parts();
        let on_lane = Plot {
            label: "on the lane".into(),
            position: [0.0, 0.0, waypoints[0].z],
        };
        let mut bytes = WIRE_TAG.to_vec();
        push_count(&mut bytes, waypoints.len());
        for waypoint in &waypoints {
            push_point(&mut bytes, *waypoint);
        }
        push_count(&mut bytes, 1);
        push_count(&mut bytes, on_lane.label.len());
        bytes.extend_from_slice(on_lane.label.as_bytes());
        push_point(&mut bytes, on_lane.at());
        assert!(matches!(
            Map::from_wire(&bytes),
            Err(MapWireError::Map(MapError::OnTheLane { plot, .. })) if plot == "on the lane"
        ));
    }

    /// **The largest map the caps admit fits one server event**, with room:
    /// its encoding is [`MAX_WIRE_BYTES`] exactly, and that is far inside the
    /// field a client reads.
    #[test]
    fn the_largest_map_the_caps_admit_fits_one_event() {
        let lanes = MAX_WAYPOINTS / 2;
        let lane_z = |lane: usize| (lane as f64 - 0.5 * (lanes - 1) as f64) * 3.0;
        let reach = super::super::HALF_WIDTH - 4.0;
        let mut waypoints = Vec::with_capacity(MAX_WAYPOINTS);
        for lane in 0..lanes {
            let (from, to) = if lane % 2 == 0 {
                (-reach, reach)
            } else {
                (reach, -reach)
            };
            waypoints.push(DVec3::new(from, 0.0, lane_z(lane)));
            waypoints.push(DVec3::new(to, 0.0, lane_z(lane)));
        }
        let plots = (0..MAX_PLOTS)
            .map(|at| {
                let side = if at % 2 == 0 { 1.0 } else { -1.0 };
                let mut label = format!("p{at}");
                label.extend(std::iter::repeat_n('x', MAX_LABEL_BYTES - label.len()));
                Plot {
                    label,
                    position: [side * (reach + 2.0), 0.0, lane_z(at / 2)],
                }
            })
            .collect();
        let largest = Map::new(waypoints, plots).expect("the serpentine is a map");
        let bytes = largest.to_wire();
        assert_eq!(bytes.len(), MAX_WIRE_BYTES);
        const { assert!(MAX_WIRE_BYTES < crcbl::net::codec::MAX_FIELD_BYTES) };
        assert_eq!(Map::from_wire(&bytes).expect("decodes"), largest);
    }
}
