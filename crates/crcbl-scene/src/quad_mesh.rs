//! Flat-shaded meshes built quad by quad, for the geometry a sample writes out
//! by hand: rooms, courts, plazas and pools.
//!
//! Four samples each carried a copy of this — `apps/alcove`, `apps/lantern`,
//! `apps/sundial` and `apps/tide` — and the copies agreed on everything that
//! decides a picture: four unshared vertices per quad, corners given
//! counter-clockwise seen from the normal's side and split `0 1 2, 0 2 3`, one
//! white vertex colour, and clusters from [`build_meshlets`]. What differed was
//! only which of the methods below each one needed.
//!
//! **Unshared corners** for the reason `crcbl_shaders::mesh::OPEN_BOX_VERTEX_COUNT`
//! records: a shared corner gets an averaged normal, which is the opposite of
//! what a flat face wants. A curved surface passes its own normals per corner
//! through [`QuadMesh::quad_shaded`] and [`QuadMesh::tri`].
//!
//! **The winding is checked, not trusted.** Every face is held to wind
//! counter-clockwise from the side its normals point to, which is what
//! `CullMode::Back` culls the other side of; a face wound the other way lights
//! plausibly and is simply not there from outside, which is how alcove's slot
//! walls drew for a day in 2026-09. A face that disagrees panics with its
//! corners.

use std::borrow::Cow;

use crcbl_render::scene::{Geometry, MeshDesc};
use crcbl_shaders::mesh::{self, MeshVertex};
use crcbl_shaders::vertex::UvRange;
use glam::Vec3;

use crate::meshlet::{MeshletError, build_meshlets};

/// Which way an axis-aligned face points along its axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    /// The face's normal points along `+axis`.
    Positive,
    /// The face's normal points along `-axis`.
    Negative,
}

/// The texture coordinates of a quad's four corners in the order the quad
/// methods take them: `v = 0` at the first corner, which is the one at the
/// minimum `y` for every vertical face below.
pub const QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// A flat-shaded triangle list under construction; see the [module docs](self).
#[derive(Debug)]
pub struct QuadMesh {
    vertices: Vec<Corner>,
    indices: Vec<u32>,
    /// Whether the mesh samples a page: `false` gives every vertex the same
    /// coordinate and a degenerate range, `true` gives a quad [`QUAD_UV`] and
    /// the range of every coordinate the mesh carries.
    textured: bool,
}

/// One corner as authored, on the way to a [`MeshVertex`]: floats until
/// [`QuadMesh::finish`], because a `unorm16` UV lane needs the range of every
/// coordinate the mesh carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    /// Where it is.
    pub position: Vec3,
    /// Its normal: the face's for a flat quad, its own for a shaded one.
    pub normal: Vec3,
    /// Its texture coordinate; `(0, 0)` throughout an untextured mesh.
    pub uv: [f32; 2],
}

impl QuadMesh {
    /// A mesh that samples no page: every material row it is drawn with names
    /// `GpuMaterial::NO_PAGE`, so every vertex carries the same texture
    /// coordinate and the range is degenerate on purpose.
    #[must_use]
    pub const fn untextured() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            textured: false,
        }
    }

    /// A mesh whose quads carry texture coordinates, [`QUAD_UV`] unless
    /// [`QuadMesh::quad_uv`] says otherwise.
    #[must_use]
    pub const fn textured() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            textured: true,
        }
    }

    /// One quad with one `normal`, its corners counter-clockwise seen from
    /// `normal`'s side.
    ///
    /// # Panics
    ///
    /// If the corners wind the other way — see the [module docs](self).
    pub fn quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let uvs = if self.textured {
            QUAD_UV
        } else {
            [[0.0; 2]; 4]
        };
        self.quad_uv(corners, normal, uvs);
    }

    /// [`QuadMesh::quad`] with the texture coordinates written out, for a
    /// surface that cares which way up its texture is.
    ///
    /// # Panics
    ///
    /// As [`QuadMesh::quad`].
    pub fn quad_uv(&mut self, corners: [Vec3; 4], normal: Vec3, uvs: [[f32; 2]; 4]) {
        self.push_quad(corners, [normal; 4], uvs);
    }

    /// [`QuadMesh::quad`] with a normal per corner, for a curved surface whose
    /// facets share their corners' directions with their neighbours.
    ///
    /// # Panics
    ///
    /// If any normal points away from the side the corners wind
    /// counter-clockwise from.
    pub fn quad_shaded(&mut self, corners: [Vec3; 4], normals: [Vec3; 4]) {
        let uvs = if self.textured {
            QUAD_UV
        } else {
            [[0.0; 2]; 4]
        };
        self.push_quad(corners, normals, uvs);
    }

    /// One triangle, counter-clockwise seen from its normals' side — a sphere's
    /// pole rings, where a quad would have two coincident corners. Its corners
    /// carry the coordinate `(0, 0)` whether or not the mesh is textured.
    ///
    /// # Panics
    ///
    /// As [`QuadMesh::quad_shaded`].
    pub fn tri(&mut self, corners: [Vec3; 3], normals: [Vec3; 3]) {
        facing_its_normals(&corners, &normals);
        let base = self.push_corners(&corners, &normals, &[[0.0; 2]; 3]);
        self.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }

    /// A quad in the plane `x`, spanning `y` and `z`, facing `facing`.
    pub fn quad_x(&mut self, x: f32, facing: Facing, y: (f32, f32), z: (f32, f32)) {
        let at = |y: f32, z: f32| Vec3::new(x, y, z);
        match facing {
            Facing::Positive => self.quad(
                [at(y.0, z.1), at(y.0, z.0), at(y.1, z.0), at(y.1, z.1)],
                Vec3::X,
            ),
            Facing::Negative => self.quad(
                [at(y.0, z.0), at(y.0, z.1), at(y.1, z.1), at(y.1, z.0)],
                Vec3::NEG_X,
            ),
        }
    }

    /// A quad in the plane `y`, spanning `x` and `z`, facing `facing`.
    pub fn quad_y(&mut self, y: f32, facing: Facing, x: (f32, f32), z: (f32, f32)) {
        let at = |x: f32, z: f32| Vec3::new(x, y, z);
        match facing {
            Facing::Positive => self.quad(
                [at(x.0, z.1), at(x.1, z.1), at(x.1, z.0), at(x.0, z.0)],
                Vec3::Y,
            ),
            Facing::Negative => self.quad(
                [at(x.0, z.0), at(x.1, z.0), at(x.1, z.1), at(x.0, z.1)],
                Vec3::NEG_Y,
            ),
        }
    }

    /// A quad in the plane `z`, spanning `x` and `y`, facing `facing`.
    pub fn quad_z(&mut self, z: f32, facing: Facing, x: (f32, f32), y: (f32, f32)) {
        let at = |x: f32, y: f32| Vec3::new(x, y, z);
        match facing {
            Facing::Positive => self.quad(
                [at(x.0, y.0), at(x.1, y.0), at(x.1, y.1), at(x.0, y.1)],
                Vec3::Z,
            ),
            Facing::Negative => self.quad(
                [at(x.1, y.0), at(x.0, y.0), at(x.0, y.1), at(x.1, y.1)],
                Vec3::NEG_Z,
            ),
        }
    }

    /// A closed box between `min` and `max`, every face pointing **out**, its
    /// corners at exactly `min` and `max`.
    pub fn box_outward(&mut self, min: Vec3, max: Vec3) {
        let (x, y, z) = ((min.x, max.x), (min.y, max.y), (min.z, max.z));
        self.quad_x(max.x, Facing::Positive, y, z);
        self.quad_x(min.x, Facing::Negative, y, z);
        self.quad_y(max.y, Facing::Positive, x, z);
        self.quad_y(min.y, Facing::Negative, x, z);
        self.quad_z(max.z, Facing::Positive, x, y);
        self.quad_z(min.z, Facing::Negative, x, y);
    }

    /// A closed box about `centre` in the right-handed frame `axes`, `half`
    /// along each axis, every face pointing **out** — a box that is not
    /// axis-aligned.
    ///
    /// The faces come in [`QuadMesh::box_outward`]'s order with `x`, `y` and
    /// `z` read as the frame's three axes. Its corners are `centre ± half`,
    /// which is not bit-identical to an axis-aligned box's `min` and `max`, so
    /// the two are separate methods rather than one calling the other.
    pub fn box_frame(&mut self, centre: Vec3, axes: [Vec3; 3], half: Vec3) {
        let [x, y, z] = axes;
        let at = |sx: f32, sy: f32, sz: f32| {
            centre + x * (half.x * sx) + y * (half.y * sy) + z * (half.z * sz)
        };
        self.quad(
            [
                at(1.0, -1.0, 1.0),
                at(1.0, -1.0, -1.0),
                at(1.0, 1.0, -1.0),
                at(1.0, 1.0, 1.0),
            ],
            x,
        );
        self.quad(
            [
                at(-1.0, -1.0, -1.0),
                at(-1.0, -1.0, 1.0),
                at(-1.0, 1.0, 1.0),
                at(-1.0, 1.0, -1.0),
            ],
            -x,
        );
        self.quad(
            [
                at(-1.0, 1.0, 1.0),
                at(1.0, 1.0, 1.0),
                at(1.0, 1.0, -1.0),
                at(-1.0, 1.0, -1.0),
            ],
            y,
        );
        self.quad(
            [
                at(-1.0, -1.0, -1.0),
                at(1.0, -1.0, -1.0),
                at(1.0, -1.0, 1.0),
                at(-1.0, -1.0, 1.0),
            ],
            -y,
        );
        self.quad(
            [
                at(-1.0, -1.0, 1.0),
                at(1.0, -1.0, 1.0),
                at(1.0, 1.0, 1.0),
                at(-1.0, 1.0, 1.0),
            ],
            z,
        );
        self.quad(
            [
                at(1.0, -1.0, -1.0),
                at(-1.0, -1.0, -1.0),
                at(-1.0, 1.0, -1.0),
                at(1.0, 1.0, -1.0),
            ],
            -z,
        );
    }

    /// Every corner added so far, in the order the faces added them — for a
    /// check of the geometry that needs no device.
    pub fn corners(&self) -> impl ExactSizeIterator<Item = Corner> + '_ {
        self.vertices.iter().copied()
    }

    /// The mesh this builder describes, clustered by [`build_meshlets`], its
    /// vertices white so the material row is the whole of what colours a
    /// surface.
    ///
    /// # Errors
    ///
    /// [`build_meshlets`]'s, which for faces added through the methods above
    /// is a mesh too large for a cluster table rather than a malformed list.
    pub fn finish(self, label: &'static str) -> Result<MeshDesc<'static>, MeshletError> {
        let positions: Vec<[f32; 3]> = self
            .vertices
            .iter()
            .map(|vertex| vertex.position.to_array())
            .collect();
        let clusters = build_meshlets(&positions, &self.indices)?.into_clusters();
        let uv_range = if self.textured {
            let uvs: Vec<[f32; 2]> = self.vertices.iter().map(|vertex| vertex.uv).collect();
            UvRange::from_uvs(&uvs)
        } else {
            UvRange::from_uvs(&[[0.0, 0.0]])
        };
        let vertices: Vec<MeshVertex> = self
            .vertices
            .iter()
            .map(|vertex| {
                MeshVertex::from_normal(
                    vertex.position.to_array(),
                    vertex.normal.to_array(),
                    [1.0, 1.0, 1.0, 1.0],
                    vertex.uv,
                    &uv_range,
                )
            })
            .collect();
        Ok(MeshDesc {
            label: Cow::Borrowed(label),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(mesh::vertex_bytes(&vertices)),
                uv_range,
                indices: Cow::Owned(self.indices),
                clusters,
                // No `MESH_AUTHORED_TANGENTS`: `MeshVertex::from_normal` fills
                // the frame with a stand-in that agrees with no UV
                // parameterisation, so there is no authored tangent to claim.
                flags: 0,
            },
        })
    }

    fn push_quad(&mut self, corners: [Vec3; 4], normals: [Vec3; 4], uvs: [[f32; 2]; 4]) {
        facing_its_normals(&[corners[0], corners[1], corners[2]], &normals);
        let base = self.push_corners(&corners, &normals, &uvs);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Pushes the corners and returns the index the first one landed at.
    fn push_corners(&mut self, corners: &[Vec3], normals: &[Vec3], uvs: &[[f32; 2]]) -> u32 {
        let base = u32::try_from(self.vertices.len())
            .expect("a hand-built mesh has fewer vertices than u32::MAX");
        for ((corner, normal), uv) in corners.iter().zip(normals).zip(uvs) {
            self.vertices.push(Corner {
                position: *corner,
                normal: *normal,
                uv: *uv,
            });
        }
        base
    }
}

/// Refuses a face whose winding disagrees with the normals it claims — see the
/// [module docs](self).
fn facing_its_normals(corners: &[Vec3; 3], normals: &[Vec3]) {
    let geometric = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
    for normal in normals {
        assert!(
            geometric.dot(*normal) > 0.0,
            "a face at {corners:?} winds clockwise seen from its normal {normal:?}, so it \
             would be culled from the side it claims to face"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(desc: &MeshDesc<'_>) -> (usize, usize) {
        match &desc.geometry {
            Geometry::Flat {
                vertices, indices, ..
            } => (vertices.len() / mesh::VERTEX_STRIDE, indices.len()),
            other => panic!("a quad mesh is flat: {other:?}"),
        }
    }

    #[test]
    fn a_box_is_six_unshared_quads_of_two_triangles() {
        let mut mesh = QuadMesh::untextured();
        mesh.box_outward(Vec3::ZERO, Vec3::ONE);
        let desc = mesh.finish("box").expect("a box clusters");
        assert_eq!(flat(&desc), (24, 36));
    }

    #[test]
    fn every_axis_quad_winds_toward_its_normal_either_way() {
        // Each call panics on a wrong winding, so running them is the check.
        let mut mesh = QuadMesh::untextured();
        for facing in [Facing::Positive, Facing::Negative] {
            mesh.quad_x(0.0, facing, (0.0, 1.0), (0.0, 1.0));
            mesh.quad_y(0.0, facing, (0.0, 1.0), (0.0, 1.0));
            mesh.quad_z(0.0, facing, (0.0, 1.0), (0.0, 1.0));
        }
        let turned = Vec3::new(1.0, 0.0, 1.0).normalize();
        mesh.box_frame(
            Vec3::ZERO,
            [turned, Vec3::Y, turned.cross(Vec3::Y)],
            Vec3::ONE,
        );
        assert_eq!(flat(&mesh.finish("quads").expect("clusters")).0, 48);
    }

    #[test]
    #[should_panic(expected = "winds clockwise")]
    fn a_quad_wound_away_from_its_normal_is_refused() {
        let mut mesh = QuadMesh::untextured();
        mesh.quad(
            [Vec3::ZERO, Vec3::Y, Vec3::new(1.0, 1.0, 0.0), Vec3::X],
            Vec3::Z,
        );
    }

    #[test]
    fn a_textured_mesh_spans_its_coordinates_and_an_untextured_one_does_not() {
        let quad = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y];
        let range = |mut mesh: QuadMesh| {
            mesh.quad(quad, Vec3::Z);
            match mesh.finish("uv").expect("clusters").geometry {
                Geometry::Flat { uv_range, .. } => uv_range,
                other => panic!("flat: {other:?}"),
            }
        };
        assert_eq!(range(QuadMesh::textured()), UvRange::from_uvs(&QUAD_UV));
        assert_eq!(
            range(QuadMesh::untextured()),
            UvRange::from_uvs(&[[0.0, 0.0]])
        );
    }
}
