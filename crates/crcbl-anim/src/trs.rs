//! The translation/rotation/scale triple a joint's local transform is spelled
//! in, and its one composition rule.

use glam::{Mat4, Quat, Vec3};

/// A local transform as its three animatable components.
///
/// glTF calls these the *TRS properties*, and animation targets them
/// individually: a channel drives `translation`, `rotation` or `scale`, never a
/// matrix. Keeping the three apart is therefore not a storage preference but
/// the only shape a sampler can write into — a matrix pose would have to be
/// decomposed and recomposed once per channel per frame.
///
/// The specification's own words, from §3.7.3.2 (Transformations):
///
/// > Any node **MAY** define a local space transform either by supplying a
/// > `matrix` property, or any of `translation`, `rotation`, and `scale`
/// > properties (also known as *TRS properties*). […] When a node is targeted
/// > for animation (referenced by an `animation.channel.target`), only TRS
/// > properties **MAY** be present; `matrix` **MUST NOT** be present.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    /// The translation, in the document's own units.
    pub translation: Vec3,
    /// The rotation, as a unit quaternion.
    pub rotation: Quat,
    /// The scale, per axis.
    pub scale: Vec3,
}

impl Trs {
    /// No translation, no rotation, unit scale — the specification's default
    /// for a node that declares none of the three.
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    /// The local transformation matrix these three compose to.
    ///
    /// From §3.7.3.2:
    ///
    /// > To compose the local transformation matrix, TRS properties **MUST** be
    /// > converted to matrices and postmultiplied in the `T * R * S` order;
    /// > first the scale is applied to the vertices, then the rotation, and
    /// > then the translation.
    ///
    /// [`Mat4::from_scale_rotation_translation`] is exactly that product, and
    /// is used rather than three matrices multiplied here so the ordering lives
    /// in one place instead of being restated at every call.
    #[inline]
    #[must_use]
    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// The three components of a transformation matrix that has them.
    ///
    /// The inverse of [`to_mat4`](Self::to_mat4), and the bridge a converter
    /// needs: an imported joint's rest pose arrives as a matrix
    /// (`crcbl_scene::GltfNode::local_transform`) even when the file spelled it
    /// as TRS, because a node **MAY** use either. The decomposition is sound on
    /// exactly the matrices glTF permits there — §3.7.3.2 again:
    ///
    /// > When `matrix` is defined, it **MUST** be decomposable to TRS
    /// > properties.
    ///
    /// with the implementation note that "transformation matrices cannot skew
    /// or shear". A matrix that does shear is not a glTF node transform, and
    /// what comes back for one is glam's best effort rather than an error:
    /// there is no honest triple to return.
    #[inline]
    #[must_use]
    pub fn from_mat4(matrix: Mat4) -> Self {
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        Self {
            translation,
            rotation,
            scale,
        }
    }
}

impl Default for Trs {
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::Trs;
    use glam::{Mat4, Quat, Vec3};

    /// `T * R * S` and not `S * R * T`: the two disagree the moment translation
    /// and scale are both non-trivial, and this case is the difference.
    ///
    /// Scale 2, no rotation, translation `(1, 0, 0)`. `T * R * S` sends the
    /// local point `(1, 0, 0)` to `(3, 0, 0)` — scaled first, then moved. The
    /// other order would scale the translation too and answer `(4, 0, 0)`.
    #[test]
    fn composes_translation_after_scale() {
        let trs = Trs {
            translation: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(2.0),
        };
        let point = trs.to_mat4().transform_point3(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(point, Vec3::new(3.0, 0.0, 0.0));
    }

    /// Rotation is applied before translation, so the translation is *not*
    /// rotated. A 90° turn about `+Z` with a `(1, 0, 0)` offset puts the local
    /// origin at `(1, 0, 0)`, and the local `+X` unit at `(1, 1, 0)`.
    #[test]
    fn composes_translation_after_rotation() {
        let trs = Trs {
            translation: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: Vec3::ONE,
        };
        let matrix = trs.to_mat4();
        let origin = matrix.transform_point3(Vec3::ZERO);
        let unit_x = matrix.transform_point3(Vec3::new(1.0, 0.0, 0.0));
        assert!((origin - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-6);
        assert!((unit_x - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn round_trips_through_a_matrix() {
        let trs = Trs {
            translation: Vec3::new(-3.0, 0.5, 7.0),
            rotation: Quat::from_rotation_y(0.75),
            scale: Vec3::new(2.0, 2.0, 2.0),
        };
        let back = Trs::from_mat4(trs.to_mat4());
        assert!((back.translation - trs.translation).length() < 1e-5);
        assert!((back.scale - trs.scale).length() < 1e-5);
        assert!(back.rotation.dot(trs.rotation).abs() > 1.0 - 1e-6);
    }

    #[test]
    fn identity_is_the_specification_default() {
        assert_eq!(Trs::default(), Trs::IDENTITY);
        assert_eq!(Trs::IDENTITY.to_mat4(), Mat4::IDENTITY);
    }
}
