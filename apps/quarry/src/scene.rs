//! The generated face, in the vocabulary [`ForwardRenderer`] makes resident.
//!
//! [`crate::face`] ends at host arrays: positions, normals and indices. The
//! renderer starts somewhere else — vertex *bytes* in `crcbl_shaders::mesh`'s
//! layout, meshlet clusters, a material table and a texture page — and this is
//! the join, the same one `crcbl_scene::gltf_render` makes for a `.glb`.
//!
//! # The pool sizes are computed, and that is not a detail
//!
//! [`Capacities::default`] reserves 65,536 vertices and 262,144 indices, which
//! is generous for the demo cube and **too small for this sample's own
//! content**: the face at 256 cells is 66,049 vertices and 393,216 indices, so
//! the defaults miss on both counts. A `SceneDesc` that took them would be
//! refused by
//! [`with_scene`](crcbl::render::ForwardRenderer::with_scene) at start-up, and
//! `the_default_capacities_would_not_fit_the_face` is the test that keeps this
//! module honest about why it does its own arithmetic.
//!
//! [`ForwardRenderer`]: crcbl::render::ForwardRenderer

use std::borrow::Cow;

use crcbl::render::scene::{Capacities, Geometry, MeshDesc, PageDesc, ProbeGrid, SceneDesc};
use crcbl::scene::meshlet::{MeshletError, build_meshlets};
use crcbl::shaders::mesh::{GpuMaterial, MeshVertex};
use crcbl::shaders::vertex::UvRange;

use crate::face::Face;

/// The one material every cluster shades through: dry, unpolished rock.
///
/// Rough and fully dielectric, so the surface reads by its geometry rather than
/// by a highlight — this sample's subject is the mesh, and a glossy face would
/// hide the thing it exists to show.
pub(crate) const ROCK: GpuMaterial = GpuMaterial {
    base_color: [0.42, 0.39, 0.35, 1.0],
    metallic: 0.0,
    roughness: 0.9,
    ..GpuMaterial::UNTINTED
};

/// Texels a side of the page. One, because nothing here samples a texture: the
/// page exists because a material table indexes one, and the smallest legal
/// page is the honest size for a scene with no images.
pub(crate) const PAGE_EXTENT: u32 = 1;

/// Headroom over the exact counts, as a multiplier.
///
/// The pools do not grow — [`SceneDesc`] is read once at `with_scene` — so a
/// scene sized exactly to today's content leaves nothing for the second mesh
/// milestone 4 adds. A quarter is enough for that and small enough that the
/// reservation still tracks the content rather than a round number somebody
/// picked.
pub(crate) const HEADROOM: f32 = 1.25;

/// The face as a scene of one mesh and one instance's worth of room.
///
/// # Errors
///
/// [`MeshletError`] if the face cannot be partitioned into meshlets, which for
/// generated content means [`crate::face`] produced something malformed rather
/// than a limit being hit.
pub fn quarry_scene(face: &Face) -> Result<SceneDesc<'static>, MeshletError> {
    let clusters = build_meshlets(&face.positions, &face.indices)?.into_clusters();
    let vertices = vertex_bytes(&face.positions, &face.normals);

    Ok(SceneDesc {
        meshes: vec![MeshDesc {
            label: Cow::Borrowed("quarry face"),
            geometry: Geometry::Flat {
                vertices: Cow::Owned(vertices),
                uv_range: UV_RANGE,
                indices: Cow::Owned(face.indices.clone()),
                clusters,
                // No `MESH_AUTHORED_TANGENTS`: `vertex_bytes` below builds
                // every vertex with `MeshVertex::from_normal`, whose frame is
                // `orthonormal_basis`' stand-in rather than an authored one.
                flags: 0,
            },
        }],
        // Row 0 is what an instance written without a material id names, so the
        // rock has to be the first row rather than merely present.
        materials: vec![ROCK],
        page: PageDesc::opaque_white(PAGE_EXTENT),
        probes: ProbeGrid::default(),
        capacities: capacities_for(face),
    })
}

/// Pool sizes this face actually needs, with a quarter of headroom.
///
/// Public because the reservation is a number this sample's exit criteria ask
/// to be recorded, and a caller that could not read it would have to re-derive
/// it from the same arithmetic.
#[must_use]
pub fn capacities_for(face: &Face) -> Capacities {
    let scale = |count: usize| -> u32 {
        let padded = (count as f32 * HEADROOM).ceil();
        // `as` saturates at the maximum for a float above it, which is the
        // behaviour wanted here: a face too large for a `u32` pool is refused by
        // `with_scene` naming the pool, rather than wrapping to a small one that
        // is accepted and overruns.
        padded as u32
    };
    Capacities {
        vertices: scale(face.positions.len()),
        indices: scale(face.indices.len()),
        meshes: 1,
        instances: 1,
        materials: 1,
        // One row for the sun, which is the only light this milestone sets. A
        // light that overflowed would be refused rather than counted, so the
        // reservation has to grow with the scene rather than be trimmed to it.
        lights: 1,
        // No probes: the surface is lit by the sun and the flat ambient term.
        // The table still holds one cleared row, which reads as nothing — see
        // `Capacities::probes`.
        probes: 0,
    }
}

/// Positions and normals in [`MeshVertex`] order, little-endian.
///
/// The same packing `crcbl_scene::gltf_render` does, and it is written out
/// rather than shared because that one is private to a crate whose subject is
/// glTF — this sample has no document.
///
/// Takes the two arrays rather than a [`Face`] because [`crate::dag`] packs a
/// *level*, whose positions came out of the decimator and belong to no face.
///
/// # Panics
///
/// If the two arrays are different lengths, which would otherwise pack a short
/// vertex buffer and be refused a level later with the pool named instead of
/// the mistake.
pub(crate) fn vertex_bytes(positions: &[[f32; 3]], normals: &[[f32; 3]]) -> Vec<u8> {
    assert_eq!(
        positions.len(),
        normals.len(),
        "a vertex needs both a position and a normal"
    );
    let vertices: Vec<MeshVertex> = positions
        .iter()
        .zip(normals)
        .map(|(position, normal)| {
            MeshVertex::from_normal(*position, *normal, ROCK.base_color, [0.0; 2], &UV_RANGE)
        })
        .collect();
    crcbl::shaders::mesh::vertex_bytes(&vertices)
}

/// The range every coordinate of this sample is quantised against: the
/// degenerate one at the origin.
///
/// Nothing here samples the page — it is one white texel — so every vertex
/// carries the same coordinate, and the range that reconstructs a single shared
/// coordinate *exactly* is the one whose extent is zero. See
/// `crcbl::shaders::vertex::UvRange::encode`, where the zero-extent axis is
/// what round-trips rather than what divides by zero.
const UV_RANGE: UvRange = UvRange {
    scale: [0.0; 2],
    offset: [0.0; 2],
};

/// [`UV_RANGE`], for the description a caller assembles — `crate::dag`'s levels
/// carry it too, and a level quantised against another range would sample a
/// different texel of a page that has only one.
#[must_use]
pub(crate) const fn uv_range() -> UvRange {
    UV_RANGE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::quarry_face;
    use crcbl::shaders::mesh::VERTEX_STRIDE;
    use crcbl::shaders::vertex::QTangent;

    /// Big enough to exceed [`Capacities::default`]'s vertex reservation, which
    /// is the point of `the_default_capacities_would_not_fit_the_face` below.
    const CELLS: u32 = 256;

    /// Small enough to be quick where the size is not the subject.
    const SMALL: u32 = 32;

    #[test]
    fn the_scene_holds_one_mesh_of_the_face_and_one_material() {
        let face = quarry_face(SMALL);
        let scene = quarry_scene(&face).expect("the face partitions into meshlets");

        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(scene.materials.len(), 1, "row 0 is what an instance names");
        let Geometry::Flat {
            vertices,
            indices,
            clusters,
            ..
        } = &scene.meshes[0].geometry
        else {
            panic!("milestone 1 is the flat path; the DAG arrives with milestone 2");
        };
        assert_eq!(
            vertices.len(),
            face.positions.len() * VERTEX_STRIDE,
            "one packed vertex per position",
        );
        assert_eq!(indices.as_ref(), face.indices.as_slice());
        assert!(!clusters.clusters.is_empty(), "no meshlets were built");
    }

    /// **The reserved pools fit the content**, which is the whole reason this
    /// module computes them.
    #[test]
    fn the_capacities_fit_the_face_they_were_sized_for() {
        let face = quarry_face(CELLS);
        let capacities = capacities_for(&face);
        assert!(
            capacities.vertices as usize >= face.positions.len(),
            "reserved {} vertices for {}",
            capacities.vertices,
            face.positions.len(),
        );
        assert!(
            capacities.indices as usize >= face.indices.len(),
            "reserved {} indices for {}",
            capacities.indices,
            face.indices.len(),
        );
    }

    /// **And the defaults would not**, which is why taking them would have been
    /// a start-up refusal rather than a tidy simplification.
    ///
    /// Stated as a test rather than a comment because it is the kind of claim
    /// that silently stops being true — a later `Capacities::default` raised to
    /// fit some other sample would make this module's arithmetic look like
    /// ceremony, and this is what would say so.
    #[test]
    fn the_default_capacities_would_not_fit_the_face() {
        let face = quarry_face(CELLS);
        let default = Capacities::default();
        assert!(
            (default.vertices as usize) < face.positions.len()
                || (default.indices as usize) < face.indices.len(),
            "the default pools ({} vertices, {} indices) now fit this face \
             ({} vertices, {} indices), so `capacities_for` no longer earns its \
             place — delete it and take the defaults",
            default.vertices,
            default.indices,
            face.positions.len(),
            face.indices.len(),
        );
    }

    /// The packed bytes really are the positions and normals the face carries,
    /// read back at the stride the shader uses.
    ///
    /// The position exactly, because stream 0 is `f32` and nothing quantises
    /// it; the normal to within the frame encoding's stated error, because the
    /// record carries a `snorm16` quaternion and the normal is what it decodes
    /// to.
    #[test]
    fn the_first_vertex_round_trips_through_the_packing() {
        let face = quarry_face(SMALL);
        let bytes = vertex_bytes(&face.positions, &face.normals);
        let vertex =
            MeshVertex::from_bytes(bytes[..VERTEX_STRIDE].try_into().expect("one whole record"));
        assert_eq!(vertex.position, face.positions[0]);
        let decoded = vertex.qtangent.decode().normal;
        for (axis, want) in decoded.iter().zip(face.normals[0]) {
            assert!(
                (axis - want).abs() <= QTangent::MAX_COMPONENT_ERROR,
                "the frame decodes to {decoded:?}, not {:?}",
                face.normals[0]
            );
        }
        // And the coordinate every vertex shares comes back exactly, which is
        // what a zero-extent range is for.
        assert_eq!(UV_RANGE.decode(vertex.uv0), [0.0; 2]);
    }
}
