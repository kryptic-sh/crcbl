//! A placing component's rotation, as a scene file carries it: see
//! [`Rotation`].

use std::fmt;

use glam::{DQuat, DVec4};
use serde::{Deserialize, Serialize};

use crcbl_reflect::Reflect;

/// How far a file's quaternion may be from unit length and still load: one
/// part in ten thousand.
///
/// Loose enough for four significant digits typed by hand — `(0.0, 0.7071,
/// 0.0, 0.7071)` is about `1e-5` short — and tight enough that a length
/// further off is a typo rather than rounding. The writer's own output is unit
/// to within a few ULPs, far inside it.
pub const ROTATION_TOLERANCE: f64 = 1.0e-4;

/// A placing component's orientation, as a scene file carries it: a unit
/// quaternion, checked where it enters, and left out of the file while it is
/// the identity.
///
/// # Stored as a quaternion, written as four numbers
///
/// A row spells it `rotation: (x, y, z, w)` — glam's component order, the
/// order [`glam::DQuat::from_array`] reads — beside the `position: (x, y, z)`
/// every placing component already has. Four plain numbers rather than glam's
/// own serde form, which needs a glam feature the workspace does not turn on,
/// and rather than Euler angles, which are a view: three angles have several
/// spellings of one orientation and lose an axis at a right-angle pitch, so a
/// file of them would not read back to the orientation that was saved. The
/// inspector draws a rotation as three angles all the same — that is a
/// per-type row over this value, and the value stays a quaternion.
///
/// # Checked where it enters, by name
///
/// A file's four numbers are refused, not repaired, when they are not a
/// rotation: [`RotationError::NotFinite`] for a NaN or an infinity, and
/// [`RotationError::NotUnit`] for a length further than
/// [`ROTATION_TOLERANCE`] from one. Refused rather than normalised, because a
/// length of `0.5` or `3` is a typo rather than rounding, and normalising it
/// would save back as numbers nobody wrote; a value within the tolerance is
/// kept **exactly as written**, so a hand-typed `0.7071` loads and saves as
/// itself. Every read of the orientation ([`Rotation::quat`]) normalises, so a
/// value inside the tolerance still turns a box rigidly.
///
/// # A reflected write can still leave it off unit
///
/// The four numbers are reflected leaves, so a path write can set one of them
/// alone — no edit the editor makes does (its handle and its inspector row
/// write all four as one command), and nothing checks a write when it lands:
/// `#[reflect(min, max)]` is advisory, as `docs/backlog.md` records for
/// `Body`'s mass. So [`Rotation::quat`] reads any length as its direction, and
/// the one value with no direction — all four zero — as the identity, while a
/// save writes the value as it stands and the next load refuses it by name.
///
/// # How a component carries one
///
/// Reflected as four leaves, `x`, `y`, `z` and `w`, which is what an edit's
/// path names (`rotation.w`). The [`Default`] is the identity, and a row
/// leaves the field out while it holds exactly that
/// (`#[serde(default, skip_serializing_if = "Rotation::is_identity")]` on the
/// component's field), so a scene with nothing turned is the file it was
/// before rotations existed.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl_reflect")]
#[serde(try_from = "[f64; 4]", into = "[f64; 4]")]
pub struct Rotation {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

impl Rotation {
    /// No turn at all.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// The reflected leaves, in the order a path names them and the order a
    /// file writes them: `rotation.x` to `rotation.w`.
    pub const LEAVES: [&'static str; 4] = ["x", "y", "z", "w"];

    /// `quat` as a rotation, its four numbers kept as they are.
    ///
    /// # Errors
    ///
    /// [`RotationError`] for a quaternion that is not finite or not unit to
    /// within [`ROTATION_TOLERANCE`] — what a file holding it would be refused
    /// for.
    pub fn new(quat: DQuat) -> Result<Self, RotationError> {
        Self::try_from(quat.to_array())
    }

    /// The orientation, normalised: what a placement turns its box by.
    ///
    /// All four numbers zero — reachable only by reflected writes (see
    /// [`Rotation`]) — has no direction and reads as
    /// [`DQuat::IDENTITY`].
    #[must_use]
    pub fn quat(&self) -> DQuat {
        DVec4::new(self.x, self.y, self.z, self.w)
            .try_normalize()
            .map_or(DQuat::IDENTITY, DQuat::from_vec4)
    }

    /// The four numbers as they are stored, `[x, y, z, w]`.
    #[must_use]
    pub const fn to_array(&self) -> [f64; 4] {
        [self.x, self.y, self.z, self.w]
    }

    /// Whether this is exactly [`Rotation::IDENTITY`] — the value a row leaves
    /// out of its file.
    ///
    /// Exactly, not "turns nothing": `(0, 0, 0, -1)` turns nothing too and is
    /// written, so what is omitted is always what [`Default`] reads back.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// `Ok` for a value a file could hold, or why a load would refuse it.
    ///
    /// # Errors
    ///
    /// See [`Rotation::new`].
    pub fn check(&self) -> Result<(), RotationError> {
        let numbers = self.to_array();
        if !numbers.iter().all(|value| value.is_finite()) {
            return Err(RotationError::NotFinite(numbers));
        }
        let length = DVec4::from_array(numbers).length();
        if (length - 1.0).abs() > ROTATION_TOLERANCE {
            return Err(RotationError::NotUnit { length });
        }
        Ok(())
    }
}

impl Default for Rotation {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl TryFrom<[f64; 4]> for Rotation {
    type Error = RotationError;

    fn try_from([x, y, z, w]: [f64; 4]) -> Result<Self, RotationError> {
        let rotation = Self { x, y, z, w };
        rotation.check()?;
        Ok(rotation)
    }
}

impl From<Rotation> for [f64; 4] {
    fn from(rotation: Rotation) -> Self {
        rotation.to_array()
    }
}

/// Four numbers that are not a rotation, naming why.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RotationError {
    /// A component is a NaN or an infinity.
    NotFinite([f64; 4]),
    /// The quaternion's length is further than [`ROTATION_TOLERANCE`] from
    /// one.
    NotUnit {
        /// Its length.
        length: f64,
    },
}

impl fmt::Display for RotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite(numbers) => write!(
                f,
                "a `rotation` must be four finite numbers, not {numbers:?}"
            ),
            Self::NotUnit { length } => write!(
                f,
                "a `rotation` must be a unit quaternion (x, y, z, w), within \
                 {ROTATION_TOLERANCE} of length 1, not one of length {length}"
            ),
        }
    }
}

impl std::error::Error for RotationError {}

#[cfg(test)]
mod tests;
