//! The v2 vertex's attribute encodings, host side.
//!
//! `docs/plan/43-render-standards.md` §2's 2026-08-30 decision splits the
//! vertex into a position stream and an attribute stream, and pays for the
//! split by narrowing every attribute: the normal and the tangent become one
//! `snorm16x4` quaternion, each UV pair becomes `unorm16x2` over a per-mesh
//! range, and the colour becomes `rgba8`. This module is the arithmetic that
//! produces those lanes and reads them back.
//!
//! # Why it is a module of its own
//!
//! Every place that constructs a vertex today — the plan's "Who constructs a
//! vertex today" list — would otherwise carry its own copy of a quaternion
//! extraction and two quantisations. One tested module means the slice that
//! replaces `MeshVertex` calls into arithmetic that already has known values
//! behind it, and that the Slang decode written beside it can be held to the
//! same ones.
//!
//! Beside [`mesh`](crate::mesh) rather than inside it because that module is
//! the largest file in the crate already.
//!
//! # What is here and what is not
//!
//! The encodings alone. Nothing here changes
//! [`MeshVertex`](crate::mesh::MeshVertex), the shader copies of it, or any
//! constructor; that is the next slice, and it is what turns these functions
//! from tested arithmetic into the format on the wire.

/// The scale a snorm16 lane is quantised by, and the magnitude a decoded lane
/// of `-32768` is clamped to.
///
/// `i16::MAX` rather than `32768`: every API that reads these bytes — Vulkan's
/// `VK_FORMAT_R16G16B16A16_SNORM`, D3D's `R16G16B16A16_SNORM`, WebGPU's
/// `snorm16x4` — decodes `n / 32767` clamped to `[-1, 1]`, which is what makes
/// both `+1.0` and `-1.0` exactly representable.
const SNORM_SCALE: f32 = i16::MAX as f32;

/// The scale a unorm16 lane is quantised by: `n / 65535`, so `0.0` and `1.0`
/// are both exact.
const UNORM_SCALE: f32 = u16::MAX as f32;

/// An orthonormal tangent frame, the three vectors a [`QTangent`] stands in
/// for.
///
/// Right-handed when `dot(cross(normal, tangent), bitangent)` is positive,
/// which is the sign glTF carries in its tangent's `w` and the sign a
/// [`QTangent`] carries in its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TangentFrame {
    /// The surface tangent, unit length.
    pub tangent: [f32; 3],
    /// The surface bitangent, unit length and perpendicular to both others.
    pub bitangent: [f32; 3],
    /// The surface normal, unit length.
    pub normal: [f32; 3],
}

/// A tangent frame as a unit quaternion in four snorm16 lanes, with the
/// frame's handedness in the sign of `w`.
///
/// Crytek's QTangents — Schultz, "Spherical Skinning with Dual-Quaternions and
/// QTangents", SIGGRAPH 2011 course — transcribed. Eight bytes carry what a
/// `float3` normal and a `float4` tangent carry in twenty-eight, and the
/// vertex stream that has to be re-fetched per lit tile is the one that pays
/// for the difference.
///
/// # Which axis is which
///
/// The quaternion is the rotation that takes the canonical axes to the frame,
/// so the decode is three rotations of a fixed vector:
///
/// | Canonical axis | Basis vector |
/// | -------------- | ------------ |
/// | `(1, 0, 0)`    | tangent      |
/// | `(0, 1, 0)`    | bitangent, before the handedness sign |
/// | `(0, 0, 1)`    | normal       |
///
/// [`decode`](Self::decode) recovers the bitangent as
/// `handedness × cross(normal, tangent)` rather than by rotating `(0, 1, 0)`:
/// the two agree for a right-handed frame, and only the first is right for a
/// left-handed one, because the quaternion it was built from was the
/// right-handed frame's. The Slang decode written in the slice that wires this
/// up mirrors the same three lines.
///
/// # Why `w` can never quantise to zero
///
/// The handedness rides in the *sign* of `w`, and snorm16 has no signed zero —
/// a frame whose rotation is a half turn about an axis in the `xy` plane has
/// `w = 0` exactly, and quantising it loses the sign and mirrors the
/// bitangent. The paper's answer, and this one, is the bias in
/// [`BIAS`](Self::BIAS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QTangent(pub [i16; 4]);

impl QTangent {
    /// The smallest magnitude `w` is allowed to hold before quantising: one
    /// snorm16 step, so it lands on `±1` rather than on `0`.
    ///
    /// [`encode`](Self::encode) pushes a `w` below this up to it and shortens
    /// `xyz` to keep the quaternion unit length. The cost is an error in the
    /// frame of about twice this in radians — the same order as the rounding
    /// the other three lanes already suffer, and inside
    /// [`MAX_COMPONENT_ERROR`](Self::MAX_COMPONENT_ERROR). The alternative is
    /// a bitangent pointing the wrong way on every vertex whose frame is near
    /// a half turn.
    pub const BIAS: f32 = 1.0 / SNORM_SCALE;

    /// The largest a decoded basis vector's component may differ from the one
    /// encoded, over a frame that is orthonormal going in.
    ///
    /// Quantising a unit quaternion moves each lane by up to half a step, and
    /// the rotation it describes turns a basis vector by roughly twice the
    /// angle that costs — so this is a few snorm16 steps and not one. Measured
    /// over the sweep in `a_frame_survives_the_round_trip_in_every_octant`,
    /// which is also what would report it drifting.
    pub const MAX_COMPONENT_ERROR: f32 = 1.0e-4;

    /// Encode an orthonormal frame.
    ///
    /// The handedness comes from `dot(cross(normal, tangent), bitangent)`, and
    /// the quaternion is built from the *right-handed* frame either way — a
    /// left-handed frame's matrix has determinant `-1` and is not a rotation at
    /// all, so there is no quaternion for it to extract. What the sign of `w`
    /// then records is which of the two the caller handed over.
    ///
    /// A frame that is not orthonormal going in is encoded as though it were:
    /// the extraction reads the matrix its three columns make, and a
    /// non-rotation makes a quaternion that is normalised into some nearby
    /// rotation rather than rejected. [`orthonormal_basis`] is what a caller
    /// with no tangent at all should build the frame with.
    pub fn encode(frame: TangentFrame) -> Self {
        let handedness = if dot(cross(frame.normal, frame.tangent), frame.bitangent) < 0.0 {
            -1.0
        } else {
            1.0
        };
        // Columns `(tangent, bitangent, normal)`, with the bitangent flipped
        // where that is what makes the three a rotation.
        let mut q = normalise4(quaternion_from_columns(
            frame.tangent,
            scale3(frame.bitangent, handedness),
            frame.normal,
        ));

        // `q` and `-q` are the same rotation, so the sign is free to carry
        // something else — but only once it has been driven to a known one
        // first.
        if q[3] < 0.0 {
            q = [-q[0], -q[1], -q[2], -q[3]];
        }
        if q[3] < Self::BIAS {
            // Shorten `xyz` to what a `w` of exactly `BIAS` leaves for it,
            // rather than the paper's `sqrt(1 - BIAS²)` factor, which assumes
            // the `w` being replaced was zero.
            let xyz = (1.0 - Self::BIAS * Self::BIAS).sqrt() / (1.0 - q[3] * q[3]).sqrt();
            q = [q[0] * xyz, q[1] * xyz, q[2] * xyz, Self::BIAS];
        }
        if handedness < 0.0 {
            q = [-q[0], -q[1], -q[2], -q[3]];
        }

        Self([
            quantise_snorm(q[0]),
            quantise_snorm(q[1]),
            quantise_snorm(q[2]),
            quantise_snorm(q[3]),
        ])
    }

    /// `+1.0` for a right-handed frame, `-1.0` for a left-handed one: the sign
    /// [`encode`](Self::encode) put in `w`.
    ///
    /// The number a shader multiplies its reconstructed bitangent by, and the
    /// number glTF stores in its tangent's fourth component.
    pub fn handedness(self) -> f32 {
        if self.0[3] < 0 { -1.0 } else { 1.0 }
    }

    /// Decode the frame back.
    ///
    /// Renormalises first: four independently rounded lanes are not a unit
    /// quaternion, and rotating by one that is not scales the basis vectors it
    /// produces.
    pub fn decode(self) -> TangentFrame {
        let q = normalise4([
            dequantise_snorm(self.0[0]),
            dequantise_snorm(self.0[1]),
            dequantise_snorm(self.0[2]),
            dequantise_snorm(self.0[3]),
        ]);
        let tangent = rotate(q, [1.0, 0.0, 0.0]);
        let normal = rotate(q, [0.0, 0.0, 1.0]);
        TangentFrame {
            tangent,
            bitangent: scale3(cross(normal, tangent), self.handedness()),
            normal,
        }
    }
}

/// One mesh's UV bounds, the scale and offset a `unorm16x2` coordinate is
/// reconstructed through.
///
/// The four numbers that ride in the draw constants: a UV lane holds
/// `(uv - offset) / scale` quantised, and the shader reads back
/// `offset + scale × lane`. Per mesh rather than global because a mesh that
/// tiles its texture forty times and one that does not would otherwise share
/// a range forty times wider than either needs, and the narrow one would spend
/// most of its sixteen bits on coordinates it never reaches.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UvRange {
    /// The extent of the mesh's coordinates on each axis: `max - min`, and
    /// zero on an axis where every coordinate is the same.
    pub scale: [f32; 2],
    /// The mesh's smallest coordinate on each axis.
    pub offset: [f32; 2],
}

impl UvRange {
    /// The largest a decoded coordinate differs from the one encoded, as a
    /// fraction of that axis's [`scale`](Self::scale).
    ///
    /// One step of the 65535-step grid: half of it is the rounding
    /// [`encode`](Self::encode) does, and the rest is slack for the `f32`
    /// arithmetic on both sides of the trip. Asserted by
    /// `every_coordinate_decodes_within_the_error_the_type_states`.
    ///
    /// **Relative to the extent, not to the coordinate.** A range whose offset
    /// dwarfs its extent — coordinates around 10000 spanning a hundredth of a
    /// unit — reconstructs no better than `f32` spaces its values at that
    /// magnitude, which is a property of the type the coordinate arrives in
    /// and not of this encoding.
    pub const MAX_RELATIVE_ERROR: f32 = 1.0 / UNORM_SCALE;

    /// The range that covers every coordinate in a mesh's UV list.
    ///
    /// An empty list gives the degenerate range at the origin, which encodes
    /// and decodes `[0.0, 0.0]` exactly — a mesh with no vertices has no
    /// coordinate whose reconstruction could be wrong, and the alternative is
    /// the infinite bounds an empty fold starts from reaching the draw
    /// constants.
    pub fn from_uvs(uvs: &[[f32; 2]]) -> Self {
        if uvs.is_empty() {
            return Self {
                scale: [0.0; 2],
                offset: [0.0; 2],
            };
        }
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for uv in uvs {
            for axis in 0..2 {
                min[axis] = min[axis].min(uv[axis]);
                max[axis] = max[axis].max(uv[axis]);
            }
        }
        Self {
            scale: [max[0] - min[0], max[1] - min[1]],
            offset: min,
        }
    }

    /// Quantise one coordinate onto this range.
    ///
    /// An axis of zero extent encodes to zero rather than dividing by it, so
    /// [`decode`](Self::decode) returns [`offset`](Self::offset) — which *is*
    /// the coordinate, exactly, for the mesh whose every vertex shares it.
    pub fn encode(&self, uv: [f32; 2]) -> [u16; 2] {
        let mut lanes = [0u16; 2];
        for axis in 0..2 {
            if self.scale[axis] != 0.0 {
                let unit = (uv[axis] - self.offset[axis]) / self.scale[axis];
                lanes[axis] = (unit * UNORM_SCALE).round().clamp(0.0, UNORM_SCALE) as u16;
            }
        }
        lanes
    }

    /// Read a coordinate back, the way the shader does.
    pub fn decode(&self, lanes: [u16; 2]) -> [f32; 2] {
        [
            self.offset[0] + self.scale[0] * (f32::from(lanes[0]) / UNORM_SCALE),
            self.offset[1] + self.scale[1] * (f32::from(lanes[1]) / UNORM_SCALE),
        ]
    }
}

/// A tangent and bitangent for a normal that arrived without either, so that
/// every mesh has a frame to encode.
///
/// Duff et al., "Building an Orthonormal Basis, Revisited" (JCGT 6:1, 2017),
/// the branchless form. `copysign` is what makes it branchless *and* stable:
/// the division below is by `sign + normal.z`, which the naive form lets reach
/// zero at the south pole and which this one holds at a magnitude of at least
/// one everywhere.
///
/// **This is a stand-in, not a tangent.** The frame it returns is arbitrary
/// about the normal — it agrees with no UV parameterisation, so a normal map
/// sampled through it is wrong. `docs/plan/43-render-standards.md` §2 says
/// such a mesh takes the derivative frame in the shader until the importer
/// fills a real tangent (the MikkTSpace call `docs/backlog.md` carries); what
/// this function is for is the host side of that gap, so the encoding never
/// has to represent a missing frame and every vertex has eight bytes that mean
/// something.
///
/// Returns `(tangent, bitangent)`, right-handed with the normal.
pub fn orthonormal_basis(normal: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let sign = 1.0f32.copysign(normal[2]);
    let a = -1.0 / (sign + normal[2]);
    let b = normal[0] * normal[1] * a;
    (
        [
            1.0 + sign * normal[0] * normal[0] * a,
            sign * b,
            -sign * normal[0],
        ],
        [b, sign + normal[1] * normal[1] * a, -normal[1]],
    )
}

/// Quantise a linear RGBA colour to `rgba8`.
///
/// Round to nearest, not truncate: truncation biases every channel downward by
/// half a step, which over a whole mesh is a visible darkening rather than
/// noise. A channel outside `[0, 1]`, an infinity included, is clamped into it
/// first; a `NaN` channel survives the clamp and the saturating cast makes it
/// `0`.
pub fn encode_rgba8(color: [f32; 4]) -> [u8; 4] {
    let lane = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    [
        lane(color[0]),
        lane(color[1]),
        lane(color[2]),
        lane(color[3]),
    ]
}

/// Read an `rgba8` colour back as linear floats, the way the shader's
/// `unorm8` fetch does: `n / 255`, so `0` and `255` are exactly `0.0` and
/// `1.0`.
pub fn decode_rgba8(color: [u8; 4]) -> [f32; 4] {
    [
        f32::from(color[0]) / 255.0,
        f32::from(color[1]) / 255.0,
        f32::from(color[2]) / 255.0,
        f32::from(color[3]) / 255.0,
    ]
}

fn quantise_snorm(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * SNORM_SCALE).round() as i16
}

fn dequantise_snorm(lane: i16) -> f32 {
    (f32::from(lane) / SNORM_SCALE).clamp(-1.0, 1.0)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn scale3(a: [f32; 3], by: f32) -> [f32; 3] {
    [a[0] * by, a[1] * by, a[2] * by]
}

fn normalise4(q: [f32; 4]) -> [f32; 4] {
    let length = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    [q[0] / length, q[1] / length, q[2] / length, q[3] / length]
}

/// The quaternion of the rotation whose matrix has these three as its
/// **columns**, so that it takes `(1, 0, 0)` to `x`, `(0, 1, 0)` to `y` and
/// `(0, 0, 1)` to `z`.
///
/// Shoemake's extraction: the branch with the largest denominator is the one
/// taken, because the other three lose precision as their lane approaches zero
/// and the trace branch fails outright for a half turn.
fn quaternion_from_columns(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> [f32; 4] {
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let trace = m00 + m11 + m22;
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}

/// `q v q⁻¹` for a unit `q`, in the two cross products that form costs rather
/// than the two quaternion multiplications it is written as.
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let axis = [q[0], q[1], q[2]];
    let t = scale3(cross(axis, v), 2.0);
    let wt = scale3(t, q[3]);
    let ct = cross(axis, t);
    [
        v[0] + wt[0] + ct[0],
        v[1] + wt[1] + ct[1],
        v[2] + wt[2] + ct[2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{EulerRot, Quat, Vec3};

    /// The frame a rotation makes: the canonical axes taken through it, with
    /// the bitangent flipped for a left-handed one. Built with `glam` rather
    /// than with this module's own arithmetic, so that a mistake in the
    /// extraction is not also in the expectation.
    fn frame_of(rotation: Quat, handedness: f32) -> TangentFrame {
        let tangent = rotation * Vec3::X;
        let normal = rotation * Vec3::Z;
        TangentFrame {
            tangent: tangent.to_array(),
            bitangent: (handedness * normal.cross(tangent)).to_array(),
            normal: normal.to_array(),
        }
    }

    /// The frame that needs no rotation at all, which is the one every other
    /// value here is a rotation away from — and the one whose `w` has to be
    /// the largest a snorm16 lane holds, not one step short of it.
    #[test]
    fn the_identity_frame_encodes_to_a_full_positive_w() {
        let frame = TangentFrame {
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        };
        let encoded = QTangent::encode(frame);
        assert_eq!(encoded, QTangent([0, 0, 0, i16::MAX]));
        assert_eq!(encoded.handedness(), 1.0);
        // And it decodes back to itself exactly: a unit `w` is the one value
        // in the encoding that has no rounding to lose.
        assert_eq!(encoded.decode(), frame);
    }

    /// A frame that mirrors its bitangent is the case the sign exists for, and
    /// the one a `snorm16x4` that stored the frame as three vectors could not
    /// tell from the frame above.
    #[test]
    fn a_left_handed_identity_frame_carries_its_sign_in_w() {
        let frame = TangentFrame {
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, -1.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        };
        let encoded = QTangent::encode(frame);
        assert_eq!(encoded, QTangent([0, 0, 0, -i16::MAX]));
        assert_eq!(encoded.handedness(), -1.0);
        assert_eq!(encoded.decode(), frame);
    }

    /// Known values, written out from the paper's convention rather than
    /// produced by the code they check: a frame is the canonical axes taken
    /// through a rotation, and these are the rotations whose quaternions can be
    /// written down.
    ///
    /// Without these the round-trip sweep below would pass just as happily on
    /// an extraction that swapped two axes or negated one — the frame it
    /// decoded would be the frame it encoded either way, and every consumer of
    /// the lanes would still be reading a different frame than the shader
    /// writes.
    #[test]
    fn a_frame_extracts_the_quaternion_the_paper_writes_for_it() {
        let root_half = std::f32::consts::FRAC_1_SQRT_2;
        for (name, frame, quaternion) in [
            (
                // A quarter turn about +Z: the tangent goes to +Y and the
                // bitangent to -X, the normal unmoved.
                "quarter turn about +Z",
                TangentFrame {
                    tangent: [0.0, 1.0, 0.0],
                    bitangent: [-1.0, 0.0, 0.0],
                    normal: [0.0, 0.0, 1.0],
                },
                [0.0, 0.0, root_half, root_half],
            ),
            (
                // A quarter turn about +X: the tangent unmoved, the bitangent
                // to +Z and the normal to -Y.
                "quarter turn about +X",
                TangentFrame {
                    tangent: [1.0, 0.0, 0.0],
                    bitangent: [0.0, 0.0, 1.0],
                    normal: [0.0, -1.0, 0.0],
                },
                [root_half, 0.0, 0.0, root_half],
            ),
        ] {
            let expected = QTangent([
                quantise_snorm(quaternion[0]),
                quantise_snorm(quaternion[1]),
                quantise_snorm(quaternion[2]),
                quantise_snorm(quaternion[3]),
            ]);
            assert_eq!(QTangent::encode(frame), expected, "{name}");
            // Spelled out as well, so the expectation is not only the
            // quantiser applied to a number this file also wrote.
            assert_eq!(expected.0[3], 23170, "{name}");
        }
    }

    /// A half turn about an axis in the `xy` plane has `w = 0` exactly, so it
    /// is the frame whose handedness a plain quantisation loses — `0` has no
    /// sign, and the bitangent comes back mirrored for every left-handed
    /// vertex of the mesh.
    ///
    /// [`QTangent::BIAS`] is what stops that, and this is the test that says
    /// so: **both** handednesses, because dropping the bias leaves the
    /// right-handed half passing.
    #[test]
    fn a_half_turn_frame_keeps_its_handedness_through_the_bias() {
        let mut checked = 0;
        for step in 0..16 {
            let about = step as f32 / 16.0 * std::f32::consts::TAU;
            // The axis lies in `xy`, which is what puts the half turn's `w` at
            // zero rather than merely near it.
            let axis = Vec3::new(about.cos(), about.sin(), 0.0);
            let rotation = Quat::from_axis_angle(axis, std::f32::consts::PI);
            for handedness in [1.0f32, -1.0] {
                let frame = frame_of(rotation, handedness);
                let encoded = QTangent::encode(frame);
                assert_ne!(encoded.0[3], 0, "w quantised away at {about} rad");
                assert_eq!(
                    encoded.handedness(),
                    handedness,
                    "half turn about {axis} lost its handedness"
                );
                let decoded = encoded.decode();
                for (which, want, got) in [
                    ("tangent", frame.tangent, decoded.tangent),
                    ("bitangent", frame.bitangent, decoded.bitangent),
                    ("normal", frame.normal, decoded.normal),
                ] {
                    for lane in 0..3 {
                        assert!(
                            (want[lane] - got[lane]).abs() <= QTangent::MAX_COMPONENT_ERROR,
                            "half turn about {axis}: {which} {want:?} came back {got:?}"
                        );
                    }
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 32, "the half-turn sweep did not run");
    }

    /// Every octant and both handednesses, round-tripped: the encoding is only
    /// worth its eight bytes if the frame that comes out is the frame that
    /// went in, everywhere on the sphere rather than near the axes the known
    /// values above sit on.
    ///
    /// The worst error is asserted from *both* sides. Under
    /// [`QTangent::MAX_COMPONENT_ERROR`] is the claim the constant makes; over
    /// a good fraction of it is what stops the constant being widened until
    /// nothing can fail it.
    #[test]
    fn a_frame_survives_the_round_trip_in_every_octant() {
        let steps = 13;
        let mut octants = [false; 8];
        let mut worst = 0.0f32;
        for i in 0..steps {
            for j in 0..steps {
                for k in 0..steps {
                    let angle = |n: usize| n as f32 / steps as f32 * std::f32::consts::TAU;
                    let rotation =
                        Quat::from_euler(EulerRot::XYZ, angle(i), angle(j), angle(k)).normalize();
                    for handedness in [1.0f32, -1.0] {
                        let frame = frame_of(rotation, handedness);
                        let encoded = QTangent::encode(frame);
                        assert_eq!(encoded.handedness(), handedness);
                        let decoded = encoded.decode();
                        let octant = (usize::from(frame.normal[0] < 0.0) << 2)
                            | (usize::from(frame.normal[1] < 0.0) << 1)
                            | usize::from(frame.normal[2] < 0.0);
                        octants[octant] = true;
                        for (want, got) in [
                            (frame.tangent, decoded.tangent),
                            (frame.bitangent, decoded.bitangent),
                            (frame.normal, decoded.normal),
                        ] {
                            for lane in 0..3 {
                                let error = (want[lane] - got[lane]).abs();
                                worst = worst.max(error);
                                assert!(
                                    error <= QTangent::MAX_COMPONENT_ERROR,
                                    "{want:?} came back {got:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(
            octants.iter().all(|seen| *seen),
            "the sweep never reached every octant: {octants:?}"
        );
        assert!(
            worst > QTangent::MAX_COMPONENT_ERROR / 4.0,
            "the stated error {} is far above the worst this sweep can produce ({worst:e}), so \
             nothing would fail it",
            QTangent::MAX_COMPONENT_ERROR
        );
    }

    /// The bounds are the mesh's, on each axis independently — a range taken
    /// from `u` alone would spend `v`'s sixteen bits on coordinates the mesh
    /// does not have.
    #[test]
    fn the_range_covers_exactly_the_coordinates_a_mesh_carries() {
        let uvs = [[0.25, -1.5], [0.75, 2.0], [-0.5, 0.0]];
        let range = UvRange::from_uvs(&uvs);
        assert_eq!(range.offset, [-0.5, -1.5]);
        assert_eq!(range.scale, [1.25, 3.5]);
        // The extremes land on the ends of the grid, which is what says the
        // range is tight rather than merely containing.
        assert_eq!(range.encode([-0.5, -1.5]), [0, 0]);
        assert_eq!(range.encode([0.75, 2.0]), [u16::MAX, u16::MAX]);
        // And a mesh with no vertices does not put an infinity in the draw
        // constants.
        let empty = UvRange::from_uvs(&[]);
        assert_eq!(empty.scale, [0.0, 0.0]);
        assert_eq!(empty.offset, [0.0, 0.0]);
    }

    /// [`UvRange::MAX_RELATIVE_ERROR`] is a claim about every coordinate in
    /// the range, not about the ends, so it is checked across the range —
    /// and, as above, from both sides, so the constant cannot be widened into
    /// meaninglessness.
    #[test]
    fn every_coordinate_decodes_within_the_error_the_type_states() {
        let mut worst = 0.0f32;
        for corners in [
            [[0.0f32, 0.0], [1.0, 1.0]],
            [[-4.0, -4.0], [12.0, 12.0]],
            [[0.25, 0.5], [0.3, 0.9]],
        ] {
            let range = UvRange::from_uvs(&corners);
            for step in 0..=2000 {
                let along = step as f32 / 2000.0;
                let uv = [
                    range.offset[0] + range.scale[0] * along,
                    range.offset[1] + range.scale[1] * along,
                ];
                let back = range.decode(range.encode(uv));
                for axis in 0..2 {
                    let relative = (back[axis] - uv[axis]).abs() / range.scale[axis];
                    worst = worst.max(relative);
                    assert!(
                        relative <= UvRange::MAX_RELATIVE_ERROR,
                        "{uv:?} came back {back:?} through {range:?}"
                    );
                }
            }
        }
        assert!(
            worst > UvRange::MAX_RELATIVE_ERROR / 4.0,
            "the stated error {} is far above the worst this sweep can produce ({worst:e}), so \
             nothing would fail it",
            UvRange::MAX_RELATIVE_ERROR
        );
    }

    /// A mesh whose every vertex shares a coordinate on one axis — a strip
    /// mapped down one row of an atlas — has zero extent there, and the
    /// division that normalises a coordinate onto the range has nothing to
    /// divide by. The value has to survive anyway, exactly: it is the only
    /// value that axis has.
    #[test]
    fn an_axis_with_no_extent_round_trips_exactly() {
        let uvs = [[0.0, 0.375], [1.0, 0.375], [0.5, 0.375]];
        let range = UvRange::from_uvs(&uvs);
        assert_eq!(range.scale[1], 0.0);
        for uv in uvs {
            let back = range.decode(range.encode(uv));
            assert_eq!(back[1], 0.375, "{uv:?} came back {back:?}");
        }
    }

    /// A tiling mesh's coordinates run well outside the unit square, and the
    /// range is what makes that free: `unorm16` names a fraction of the range,
    /// not a fraction of one. Clamping into `[0, 1]` instead would collapse
    /// every repeat onto the last one.
    #[test]
    fn coordinates_outside_the_unit_square_are_ranged_not_clamped() {
        let uvs = [[-8.0, 0.0], [24.0, 16.0]];
        let range = UvRange::from_uvs(&uvs);
        for uv in uvs {
            let back = range.decode(range.encode(uv));
            for axis in 0..2 {
                assert!(
                    (back[axis] - uv[axis]).abs()
                        <= range.scale[axis] * UvRange::MAX_RELATIVE_ERROR,
                    "{uv:?} came back {back:?}"
                );
            }
        }
        // The whole of the sixteen bits is spent on the range rather than on
        // the unit square inside it: the midpoint is the midpoint.
        assert_eq!(range.encode([8.0, 8.0]), [32768, 32768]);
    }

    /// The basis has to be a basis for **every** normal, including the south
    /// pole, which is where the naive construction divides by zero and where
    /// the branchless form earns its name.
    #[test]
    fn the_stand_in_basis_is_orthonormal_and_right_handed_everywhere() {
        let mut normals = vec![
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            // `z` of `-0.0`: `copysign` has to read the sign bit rather than
            // compare against zero, or the pole's branch is taken in the
            // equator's place.
            [1.0, 0.0, -0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        ];
        // Near `-z`, where `sign + normal.z` is smallest in magnitude.
        for scale in [1.0e-3f32, 1.0e-5, 1.0e-7] {
            for direction in [[scale, 0.0], [0.0, scale], [-scale, scale]] {
                let v = Vec3::new(direction[0], direction[1], -1.0).normalize();
                normals.push(v.to_array());
            }
        }
        // And a sweep of the sphere, so the general case is covered too.
        let steps = 24;
        for i in 0..steps {
            for j in 0..steps {
                let theta = (i as f32 + 0.5) / steps as f32 * std::f32::consts::PI;
                let phi = j as f32 / steps as f32 * std::f32::consts::TAU;
                normals.push([
                    theta.sin() * phi.cos(),
                    theta.sin() * phi.sin(),
                    theta.cos(),
                ]);
            }
        }
        let tolerance = 1.0e-5;
        for normal in &normals {
            let (tangent, bitangent) = orthonormal_basis(*normal);
            assert!(
                (dot(tangent, tangent) - 1.0).abs() <= tolerance,
                "tangent for {normal:?} is not unit: {tangent:?}"
            );
            assert!(
                (dot(bitangent, bitangent) - 1.0).abs() <= tolerance,
                "bitangent for {normal:?} is not unit: {bitangent:?}"
            );
            for (name, product) in [
                ("tangent·bitangent", dot(tangent, bitangent)),
                ("tangent·normal", dot(tangent, *normal)),
                ("bitangent·normal", dot(bitangent, *normal)),
            ] {
                assert!(
                    product.abs() <= tolerance,
                    "{name} for {normal:?} is {product:e}"
                );
            }
            let handed = cross(tangent, bitangent);
            for lane in 0..3 {
                assert!(
                    (handed[lane] - normal[lane]).abs() <= tolerance,
                    "the basis for {normal:?} is left-handed: {handed:?}"
                );
            }
        }
        assert!(normals.len() > steps * steps, "the sweep did not run");
    }

    /// Truncation is the mistake this would otherwise be: it darkens every
    /// channel by up to a step, which over a mesh is a shift in colour and not
    /// noise. `0.5` is where the two disagree by a whole step.
    #[test]
    fn a_colour_rounds_to_nearest_rather_than_truncating() {
        assert_eq!(encode_rgba8([0.5, 1.0 / 3.0, 0.0, 1.0]), [128, 85, 0, 255]);
        // Out of range on both sides, and the endpoints exact.
        assert_eq!(encode_rgba8([-1.0, 2.0, 0.0, 1.0]), [0, 255, 0, 255]);
        assert_eq!(
            decode_rgba8([0, 255, 128, 85]),
            [0.0, 1.0, 128.0 / 255.0, 85.0 / 255.0]
        );
        // Every byte is a fixed point of the round trip, which is what says
        // the two scales are the same one.
        for byte in 0..=u8::MAX {
            let colour = [byte, byte, byte, byte];
            assert_eq!(encode_rgba8(decode_rgba8(colour)), colour);
        }
    }
}
