//! Complete logical input records for shadow-atlas group reuse.

use super::ForwardRenderer;
use crate::cull::Frustum;
use crate::shadow;
use crcbl_shaders::mesh;

impl ForwardRenderer {
    /// Everything one group of the shadow atlas is a function of, as bytes to
    /// compare with the reading the image was last drawn from.
    ///
    /// A **group** is a cascade or a light slot's whole run of tiles — what one
    /// cull covers and what [`shadow::Cadence`] schedules. Per group rather than
    /// per atlas since the cadence rung: a lamp that swings makes its own group's
    /// reading differ and leaves every other group's alone, which is what lets
    /// the frame redraw its tiles and hold the rest.
    ///
    /// # What is in it
    ///
    /// * **The group's view blocks and its cull's frustum**, exactly the values
    ///   [`begin_frame_body`](Self::begin_frame_body) is about to write. Those
    ///   carry the cascade's or the light's matrices, the selection's atlas
    ///   rectangles and this frame's shadow LOD budgets — so a light that moved,
    ///   turned, changed radius or angle, gained or lost a tile, or landed in a
    ///   different rectangle is a different reading, and so is a camera that
    ///   moved, because a cascade is fitted to it.
    /// * **The selection eye and the instance count**, which reach the cull
    ///   beside the frustum and are in no block.
    /// * [`super::InstancePool::revision`], which is every caster: a transform, a mesh,
    ///   a material, a removal or a skinned re-point all pass through the one
    ///   write that moves it. It is conservative — it moves for writes no map
    ///   could show, and for writes only some other group's map could — and it
    ///   is never still while a byte the shadow pass draws from has changed.
    ///
    /// The bytes are the blocks' own `to_bytes` — the encoding the GPU reads —
    /// rather than the objects' memory, so no padding and no `f32` bit pattern
    /// nobody wrote is in the comparison. Two readings that differ by `-0.0`
    /// against `0.0` compare unequal and redraw, which is the safe direction.
    ///
    /// A skinned frame is **not** in here and is not meant to be: a record that
    /// merely carried "this frame skins" would still match the last skinned
    /// frame's and cache. [`ForwardRenderer::frame_skins`] is a separate veto on
    /// the answer for that reason, and it is where that limit is written down.
    ///
    /// # What is deliberately not in it
    ///
    /// * **The mesh pool.** Its buffers are written by `build_geometry` and
    ///   `residents` alone, both of which run inside
    ///   [`with_scene`](Self::with_scene) before a renderer exists — this type
    ///   exposes no way to upload or free a mesh afterwards. The one runtime
    ///   caller of the suballocator is
    ///   [`reserve_skinned`](Self::reserve_skinned), whose bytes are a skinned
    ///   region's, and those are `frame_skins`' always-redraw case.
    /// * **Whether the frame has shadows at all**, which is the atlas's *layout*
    ///   rather than any group's reading: a frame with shadows off gives every
    ///   group no views and no cull, so none of them reaches this function, and
    ///   `begin_frame_body`'s empty layout is what makes such a frame clear the
    ///   image once and cost nothing after.
    /// * **Materials, samplers and the colour pipeline.** `depth_pipeline`
    ///   names no fragment stage, so nothing the atlas holds is a function of
    ///   them.
    pub(super) fn shadow_group_record(
        &self,
        group: usize,
        views: &[(usize, usize, mesh::FrameUniforms)],
        culls: &[(usize, Frustum)],
        eye: [f32; 3],
        instance_count: u32,
    ) -> Vec<u8> {
        // A point light's cube is the longest a group gets, so this is one
        // allocation rather than a handful of regrowths. A hint, not a bound.
        let mut record = Vec::with_capacity(shadow::POINT_FACES * mesh::FRAME_UNIFORMS_SIZE);
        record.extend_from_slice(&self.instances.revision().to_le_bytes());
        record.extend_from_slice(&instance_count.to_le_bytes());
        for value in eye {
            record.extend_from_slice(&value.to_le_bytes());
        }
        for (_, view, block) in views.iter().filter(|(owner, _, _)| *owner == group) {
            record.extend_from_slice(&(*view as u32).to_le_bytes());
            record.extend_from_slice(&block.to_bytes());
        }
        for (cull, frustum) in culls.iter().filter(|(cull, _)| *cull == group) {
            record.extend_from_slice(&(*cull as u32).to_le_bytes());
            for plane in frustum.planes {
                for value in plane.to_array() {
                    record.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        record
    }
}
