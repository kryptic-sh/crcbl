//! A map's fingerprint: a number two processes compare to learn whether they
//! would play on the same field.
//!
//! A LAN joiner draws its own map — `crate::lan` says why — so a host and a
//! joiner on different maps would put one's towers on the other's plots. The
//! fingerprint is folded into the session's compatibility
//! (`crate::game::compatibility`), which is what makes a browser pass over a
//! host on another map and the handshake refuse a direct join to one.
//!
//! # A defined encoding, never the memory
//!
//! Every value goes through the byte layout below, field by field, and the
//! digest is taken over that — never over the bytes a [`Map`] happens to
//! occupy, which would fold in a `Vec`'s pointer and capacity and make two
//! equal maps differ. Positions go in as [`f64::to_bits`], so two coordinates
//! are the same exactly when their bits are; labels as their UTF-8 bytes after
//! their length, so the stream says where each stops.
//!
//! ```text
//! ENCODING_TAG
//! u32 LE  waypoint count
//!   per waypoint, in walk order:  x, y, z   (f64::to_bits, u64 LE)
//! u32 LE  plot count
//!   per plot, in list order:      u32 LE label length, label UTF-8,
//!                                 x, y, z   (f64::to_bits, u64 LE)
//! ```
//!
//! The digest is SHA-256 — the workspace's own, NIST-vector-tested
//! [`crcbl::shaders::sha256`] — and the fingerprint its first eight bytes read
//! little-endian. The order of both lists is part of the map: the path is
//! walked in waypoint order and `PlaceTower` numbers plots by position in
//! theirs, so a reordering is a different game even with the same points.

use crcbl::math::DVec3;
use crcbl::shaders::sha256::sha256;

use super::Map;

/// What every encoding starts with, and what changes when the layout does: a
/// build that encoded differently must never produce the same number for a
/// different reason. `a_changed_encoding_changes_the_committed_fields_fingerprint`
/// pins what the committed field comes to under this one.
const ENCODING_TAG: &[u8] = b"crcbl towers map v1";

impl Map {
    /// This map's fingerprint — see the module docs for what goes into it.
    #[must_use]
    pub fn fingerprint(&self) -> u64 {
        let digest = sha256(&self.encoding());
        let mut head = [0; 8];
        head.copy_from_slice(&digest[..8]);
        u64::from_le_bytes(head)
    }

    /// The bytes [`Map::fingerprint`] digests.
    fn encoding(&self) -> Vec<u8> {
        let waypoints = self.path().waypoints();
        let mut bytes = ENCODING_TAG.to_vec();
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
}

/// A length as a little-endian `u32`. Every list and label here is far below
/// its range — `Map::new` caps both lists, and a label is a scene file's
/// string — so one past it is a map nothing could have loaded.
fn push_count(bytes: &mut Vec<u8>, count: usize) {
    let count = u32::try_from(count).expect("a map's lists and labels are shorter than u32::MAX");
    bytes.extend_from_slice(&count.to_le_bytes());
}

/// A point as its three coordinates' bits, little-endian.
fn push_point(bytes: &mut Vec<u8>, point: DVec3) {
    for value in point.to_array() {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Plot;

    /// The committed field's fingerprint under [`ENCODING_TAG`]'s layout.
    ///
    /// Written out so an accidental change to the encoding — a field dropped,
    /// a byte order swapped, a length left out — is a red test here rather
    /// than two builds that silently refuse each other on the LAN. A change
    /// meant to happen bumps the tag and this value together.
    ///
    /// Checked outside this crate when it was written: the layout above packed
    /// by Python's `struct` from the committed `sys/*.ron` values and digested
    /// by `hashlib.sha256` gives the same eight bytes.
    const COMMITTED_FIELD: u64 = 0x9d7f_d7e0_2e21_296f;

    /// What a session on the committed field hand-shakes on, and so what two
    /// builds on it must agree about on the wire.
    const COMMITTED_SCHEMA: u64 = 0x9d7f_d7e0_2e75_7e3d;

    /// The committed field's waypoints and plots, taken apart so a test can
    /// change one thing and build the map again.
    fn parts() -> (Vec<DVec3>, Vec<Plot>) {
        let map = Map::built_in();
        (map.path().waypoints().to_vec(), map.plots().to_vec())
    }

    fn fingerprint(waypoints: Vec<DVec3>, plots: Vec<Plot>) -> u64 {
        Map::new(waypoints, plots)
            .expect("the change keeps the map legal")
            .fingerprint()
    }

    /// **The same map is the same number**, however it was arrived at: loaded
    /// twice, and rebuilt from its own parts.
    #[test]
    fn the_same_map_has_the_same_fingerprint() {
        let (waypoints, plots) = parts();
        assert_eq!(Map::built_in().fingerprint(), Map::built_in().fingerprint());
        assert_eq!(
            fingerprint(waypoints, plots),
            Map::built_in().fingerprint(),
            "the map rebuilt from its own parts is another number"
        );
    }

    /// **Moving one plot changes it** — by a quarter metre along one axis,
    /// which is a map whose towers stand somewhere else.
    #[test]
    fn moving_one_plot_changes_the_fingerprint() {
        let (waypoints, mut plots) = parts();
        let before = Map::built_in().fingerprint();
        plots[2].position[0] += 0.25;
        assert_ne!(fingerprint(waypoints, plots), before);
    }

    /// **Moving one waypoint changes it** — the lane runs somewhere else.
    ///
    /// The last corner, moved along the last leg, so the leg stays straight
    /// and the map stays legal.
    #[test]
    fn moving_one_waypoint_changes_the_fingerprint() {
        let (mut waypoints, plots) = parts();
        let before = Map::built_in().fingerprint();
        let last = waypoints.len() - 1;
        let along = (waypoints[last] - waypoints[last - 1]).normalize();
        waypoints[last] -= along * 0.5;
        assert_ne!(fingerprint(waypoints, plots), before);
    }

    /// **Renaming a plot changes it** — the overlay, the `[HUD]` line and the
    /// browser gate name plots by label.
    ///
    /// To a name of the same length, so it is the label's bytes that tell the
    /// two apart and not the length before them.
    #[test]
    fn renaming_a_plot_changes_the_fingerprint() {
        let (waypoints, mut plots) = parts();
        let before = Map::built_in().fingerprint();
        let renamed = plots[0].label.to_uppercase();
        assert_eq!(renamed.len(), plots[0].label.len());
        plots[0].label = renamed;
        assert_ne!(fingerprint(waypoints, plots), before);
    }

    /// **Reordering the waypoints changes it**: the same corners walked the
    /// other way is a lane whose spawn is the other's exit.
    #[test]
    fn reordering_the_waypoints_changes_the_fingerprint() {
        let (mut waypoints, plots) = parts();
        let before = Map::built_in().fingerprint();
        waypoints.reverse();
        assert_ne!(fingerprint(waypoints, plots), before);
    }

    /// **Reordering the plots changes it**: `PlaceTower` numbers plots by
    /// their place in the list, so the same pads in another order are
    /// commands that build somewhere else.
    #[test]
    fn reordering_the_plots_changes_the_fingerprint() {
        let (waypoints, mut plots) = parts();
        let before = Map::built_in().fingerprint();
        plots.swap(0, 1);
        assert_ne!(fingerprint(waypoints, plots), before);
    }

    /// **The committed field comes to [`COMMITTED_FIELD`]** — see its docs.
    #[test]
    fn a_changed_encoding_changes_the_committed_fields_fingerprint() {
        assert_eq!(
            Map::built_in().fingerprint(),
            COMMITTED_FIELD,
            "the committed field's fingerprint moved: 0x{:016x}",
            Map::built_in().fingerprint()
        );
    }

    /// **The committed field's schema hash is [`COMMITTED_SCHEMA`]**: the
    /// fingerprint folded into the base, as `crate::game::compatibility` does
    /// for every session — and the only field it changes.
    #[test]
    fn the_committed_fields_session_schema_is_pinned() {
        let base = crate::game::compatibility(&Map::built_in());
        assert_eq!(
            base.schema_hash, COMMITTED_SCHEMA,
            "0x{:016x}",
            base.schema_hash
        );
        let (mut waypoints, plots) = parts();
        waypoints.reverse();
        let other = crate::game::compatibility(&Map::new(waypoints, plots).expect("legal"));
        assert_ne!(other.schema_hash, base.schema_hash, "the map is not in it");
        assert_eq!(
            (other.protocol_version, other.engine_build_id),
            (base.protocol_version, base.engine_build_id),
            "the map moved something other than the schema"
        );
    }
}
