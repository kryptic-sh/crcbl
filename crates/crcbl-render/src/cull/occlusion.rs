//! The occlusion, small-feature and point-light face tests: the reference
//! implementation `cull.slang`'s `occlusionMain`, `lateMain` and face tags are
//! checked against.
//!
//! ```text
//!  depth prepass ──▶ DepthPyramid::reduce(Farthest) ──▶ levels 1..n
//!                                                            │
//!  instance box ──▶ project_box(view_proj) ──▶ ScreenBounds ─┴─▶ occluded
//! ```
//!
//! `docs/plan/03-gpu-driven-rendering.md` §3.3's two-phase cull, on
//! [`crate::cull`]'s terms: ordinary Rust, called by nothing in a frame, and
//! there so a test can state the answer the GPU has to reach. Every function
//! here is the shader's arithmetic in the shader's order, so a readback that
//! disagrees is a bug in one of the two rather than a rounding difference —
//! except at an exact boundary, where the four shader targets' own float
//! contractions may land either side and a test places nothing.
//!
//! # The pyramid holds the farthest depth, not the nearest
//!
//! `shaders/hiz.slang`'s reflection pyramid keeps the **nearest** surface of each
//! block, which is what a ray march skipping empty space needs. A cull needs the
//! other bound: a box is hidden only if it is behind *every* surface drawn over
//! the pixels it covers, and the farthest of those is the one to compare with.
//! Under reversed-Z nearer is larger, so the farthest is a `min` and the far
//! plane's `0.0` wins — one uncovered pixel in a block hides nothing behind it.
//! [`Reduction`] names both so a test can show the nearest one hiding what is
//! on screen.
//!
//! # Only the second phase has to be right
//!
//! The first phase tests against the **previous** frame's pyramid through the
//! previous frame's matrix, and whatever it gets wrong — a disocclusion, a
//! moved occluder, a camera cut — only decides what is drawn early. Everything
//! it marks is tested again against this frame's early depth, which holds a
//! subset of what the frame will hold, so a box hidden there is hidden in the
//! finished frame. That second test is the one the pixel-identity contract
//! rests on, and it is [`occluded`] here exactly as it is for the first.

use crcbl_shaders::cull::{
    ENTRY_FACE_SHIFT, ENTRY_OCCLUDED, ENTRY_RESCUED, FACE_COUNT, FACE_PLANES,
    OCCLUSION_DEPTH_SLACK, OCCLUSION_GUARD_TEXELS, PROJECTION_LIMIT,
};
use crcbl_shaders::mesh::{GpuInstance, GpuMesh};
use glam::{Mat3, Mat4, Vec3, Vec4};

use super::Frustum;
use crate::occlusion_cull::{level_extent, levels_for};

/// Which bound of a block a [`DepthPyramid`] level keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reduction {
    /// The farthest surface — `hiz.slang`'s `farthestMain`, and the only one an
    /// occlusion test may read.
    Farthest,
    /// The nearest surface — `hiz.slang`'s `fragmentMain`, the reflection
    /// march's.
    Nearest,
}

/// A depth pyramid on the CPU: levels `1..=levels_for(extent)` of a reversed-Z
/// depth image, each reduced from the level above exactly as `hiz.slang` does.
#[derive(Clone, Debug, PartialEq)]
pub struct DepthPyramid {
    extent: (u32, u32),
    /// `levels[n - 1]` is level `n`, row-major.
    levels: Vec<Vec<f32>>,
}

impl DepthPyramid {
    /// Reduces `depth` — `extent.0 * extent.1` texels, row-major, top row first
    /// — into every level [`levels_for`] allows.
    ///
    /// **`hiz.slang`'s reduction, tap for tap**: the destination texel's 2×2
    /// block of the level above, clamped to its extent, and a third tap along an
    /// odd axis so the last texel covers the row or column the halving floored
    /// past.
    ///
    /// # Panics
    ///
    /// If `depth` is not `extent.0 * extent.1` long.
    #[must_use]
    pub fn reduce(depth: &[f32], extent: (u32, u32), reduction: Reduction) -> Self {
        assert_eq!(
            depth.len(),
            extent.0 as usize * extent.1 as usize,
            "a depth image of {extent:?} holds that many texels"
        );
        let combine = |a: f32, b: f32| match reduction {
            Reduction::Farthest => a.min(b),
            Reduction::Nearest => a.max(b),
        };
        let mut levels: Vec<Vec<f32>> = Vec::new();
        for level in 1..=levels_for(extent) {
            let (source_width, source_height) = level_extent(extent, level - 1);
            let (width, height) = level_extent(extent, level);
            let source: &[f32] = levels.last().map_or(depth, Vec::as_slice);
            let load = |x: i64, y: i64| {
                let x = x.clamp(0, i64::from(source_width) - 1) as usize;
                let y = y.clamp(0, i64::from(source_height) - 1) as usize;
                source[y * source_width as usize + x]
            };
            let odd_x = source_width & 1 == 1;
            let odd_y = source_height & 1 == 1;
            let mut out = Vec::with_capacity(width as usize * height as usize);
            for y in 0..i64::from(height) {
                for x in 0..i64::from(width) {
                    let (bx, by) = (x * 2, y * 2);
                    let mut value = load(bx, by);
                    value = combine(value, load(bx + 1, by));
                    value = combine(value, load(bx, by + 1));
                    value = combine(value, load(bx + 1, by + 1));
                    if odd_x {
                        value = combine(value, load(bx + 2, by));
                        value = combine(value, load(bx + 2, by + 1));
                    }
                    if odd_y {
                        value = combine(value, load(bx, by + 2));
                        value = combine(value, load(bx + 1, by + 2));
                    }
                    if odd_x && odd_y {
                        value = combine(value, load(bx + 2, by + 2));
                    }
                    out.push(value);
                }
            }
            levels.push(out);
        }
        Self { extent, levels }
    }

    /// A pyramid out of levels already reduced — a GPU pyramid read back —
    /// over a prepass of `extent`, level 1 first.
    ///
    /// # Panics
    ///
    /// If `levels` is not [`levels_for`]`(extent)` long, or a level is not its
    /// [`level_extent`]'s texel count.
    #[must_use]
    pub fn from_levels(extent: (u32, u32), levels: Vec<Vec<f32>>) -> Self {
        assert_eq!(levels.len(), levels_for(extent) as usize, "a whole chain");
        for (index, level) in levels.iter().enumerate() {
            let (width, height) = level_extent(extent, index as u32 + 1);
            assert_eq!(
                level.len(),
                width as usize * height as usize,
                "level {}",
                index + 1
            );
        }
        Self { extent, levels }
    }

    /// Reduces `source` — one level of `(width, height)` texels — into the level
    /// below it, by [`reduce`](Self::reduce)'s taps: what a test holds each GPU
    /// level against the one above it with.
    #[must_use]
    pub fn reduce_level(source: &[f32], extent: (u32, u32), reduction: Reduction) -> Vec<f32> {
        let chain = Self::reduce(source, extent, reduction);
        chain.levels.into_iter().next().unwrap_or_default()
    }

    /// The prepass extent this pyramid was reduced from — its level 0.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.extent
    }

    /// How many levels it has, which is [`levels_for`] of its extent.
    #[must_use]
    pub fn levels(&self) -> u32 {
        u32::try_from(self.levels.len()).unwrap_or(u32::MAX)
    }

    /// Level `level`'s texels, row-major.
    ///
    /// # Panics
    ///
    /// If `level` is zero or past [`levels`](Self::levels).
    #[must_use]
    pub fn level(&self, level: u32) -> &[f32] {
        assert!(
            level >= 1 && level <= self.levels(),
            "level {level} of a {}-level pyramid",
            self.levels()
        );
        &self.levels[level as usize - 1]
    }

    /// Level `level` at an integer texel, which the caller has clamped.
    fn load(&self, level: u32, x: u32, y: u32) -> f32 {
        let width = level_extent(self.extent, level).0;
        self.levels[level as usize - 1][y as usize * width as usize + x as usize]
    }
}

/// An instance's box in world space: the centre and half-extent `cull.slang`'s
/// `world_box` computes, by the same conservative transform as
/// [`Aabb::transformed`](super::Aabb::transformed).
#[must_use]
pub fn world_box(instance: &GpuInstance, mesh: &GpuMesh) -> (Vec3, Vec3) {
    let min = Vec3::from_array(mesh.bounds_min);
    let max = Vec3::from_array(mesh.bounds_max);
    let local_center = (max + min) * 0.5;
    let local_extent = (max - min) * 0.5;
    let transform = Mat4::from_cols_array(&instance.transform);
    let basis = Mat3::from_mat4(transform);
    let absolute = Mat3::from_cols(basis.x_axis.abs(), basis.y_axis.abs(), basis.z_axis.abs());
    (
        transform.transform_point3(local_center),
        absolute * local_extent,
    )
}

/// A box projected onto a target: the pixel rectangle its corners span, with
/// `(0, 0)` at the top left, and the nearest reversed-Z depth any corner has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenBounds {
    /// The rectangle's top-left corner, in pixels.
    pub min: [f32; 2],
    /// Its bottom-right corner.
    pub max: [f32; 2],
    /// The largest depth of the eight corners — under reversed-Z, the nearest.
    pub nearest: f32,
}

/// Projects the box `center ± extent` through `view_projection` onto a target
/// of `target` pixels — `cull.slang`'s `project_box`.
///
/// `None` where the shader's box is invalid: a corner on or behind the eye
/// plane, in front of the near plane, or [`PROJECTION_LIMIT`] pixels or more
/// from the origin. Every caller reads that as "not hidden, not small".
#[must_use]
pub fn project_box(
    view_projection: Mat4,
    center: Vec3,
    extent: Vec3,
    target: (u32, u32),
) -> Option<ScreenBounds> {
    let (width, height) = (target.0 as f32, target.1 as f32);
    let mut bounds = ScreenBounds {
        min: [PROJECTION_LIMIT; 2],
        max: [-PROJECTION_LIMIT; 2],
        nearest: 0.0,
    };
    for corner in 0..8u32 {
        let side = Vec3::new(
            if corner & 1 != 0 { 1.0 } else { -1.0 },
            if corner & 2 != 0 { 1.0 } else { -1.0 },
            if corner & 4 != 0 { 1.0 } else { -1.0 },
        );
        let clip = view_projection * (center + side * extent).extend(1.0);
        // Named rather than negated in place, and written so a `NaN` fails:
        // every comparison with one is false, which makes the box invalid.
        let in_front = clip.w > 0.0;
        if !in_front {
            return None;
        }
        let depth = clip.z / clip.w;
        let x = (clip.x / clip.w * 0.5 + 0.5) * width;
        let y = (0.5 - clip.y / clip.w * 0.5) * height;
        let behind_near = depth <= 1.0;
        let on_the_page = x.abs() < PROJECTION_LIMIT && y.abs() < PROJECTION_LIMIT;
        if !behind_near || !on_the_page {
            return None;
        }
        bounds.min = [bounds.min[0].min(x), bounds.min[1].min(y)];
        bounds.max = [bounds.max[0].max(x), bounds.max[1].max(y)];
        bounds.nearest = bounds.nearest.max(depth);
    }
    Some(bounds)
}

/// Where [`occluded`] reads the pyramid for a box, before it compares anything:
/// the level and the inclusive texel range.
///
/// Public so a test can say *which* texels decided a verdict rather than only
/// what the verdict was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PyramidFootprint {
    /// The level read, from 1.
    pub level: u32,
    /// The first and last texel column read.
    pub columns: (u32, u32),
    /// The first and last texel row read.
    pub rows: (u32, u32),
}

/// The texels `cull.slang`'s `occluded` reads for `bounds` on a pyramid of
/// `levels` levels over `target` — `None` for a rectangle entirely off the
/// target, which is never hidden.
///
/// The rectangle is widened by [`OCCLUSION_GUARD_TEXELS`] and clamped to the
/// target; the level is the finest from 1 up at which it spans at most two
/// texels on each axis, or the deepest one there is.
#[must_use]
pub fn footprint(
    bounds: &ScreenBounds,
    target: (u32, u32),
    levels: u32,
) -> Option<PyramidFootprint> {
    let (width, height) = (
        i32::try_from(target.0).unwrap_or(i32::MAX),
        i32::try_from(target.1).unwrap_or(i32::MAX),
    );
    // The same saturating float → int the shader's `int(floor(…))` performs on
    // a value [`project_box`] has already kept inside the projection limit.
    let x0 = bounds.min[0].floor() as i32 - OCCLUSION_GUARD_TEXELS;
    let y0 = bounds.min[1].floor() as i32 - OCCLUSION_GUARD_TEXELS;
    let x1 = bounds.max[0].floor() as i32 + OCCLUSION_GUARD_TEXELS;
    let y1 = bounds.max[1].floor() as i32 + OCCLUSION_GUARD_TEXELS;
    if x1 < 0 || y1 < 0 || x0 >= width || y0 >= height {
        return None;
    }
    let left = x0.max(0) as u32;
    let top = y0.max(0) as u32;
    let right = x1.min(width - 1) as u32;
    let bottom = y1.min(height - 1) as u32;
    let mut level = 1;
    while level < levels
        && ((right >> level) - (left >> level) > 1 || (bottom >> level) - (top >> level) > 1)
    {
        level += 1;
    }
    let level_width = (target.0 >> level).max(1);
    let level_height = (target.1 >> level).max(1);
    Some(PyramidFootprint {
        level,
        columns: (
            (left >> level).min(level_width - 1),
            (right >> level).min(level_width - 1),
        ),
        rows: (
            (top >> level).min(level_height - 1),
            (bottom >> level).min(level_height - 1),
        ),
    })
}

/// Whether the box `center ± extent`, seen through `view_projection`, is hidden
/// behind what `pyramid` holds — `cull.slang`'s `occluded`, and the test both
/// occlusion phases run.
///
/// Hidden when the box's nearest corner, pushed nearer by
/// [`OCCLUSION_DEPTH_SLACK`] of itself, is still farther than the farthest depth
/// over every texel its footprint reads. Anything [`project_box`] refuses, and
/// anything off the target, is not hidden.
#[must_use]
pub fn occluded(view_projection: Mat4, center: Vec3, extent: Vec3, pyramid: &DepthPyramid) -> bool {
    if pyramid.levels() == 0 {
        return false;
    }
    let Some(bounds) = project_box(view_projection, center, extent, pyramid.extent()) else {
        return false;
    };
    let Some(footprint) = footprint(&bounds, pyramid.extent(), pyramid.levels()) else {
        return false;
    };
    let mut farthest = 1.0f32;
    for y in footprint.rows.0..=footprint.rows.1 {
        for x in footprint.columns.0..=footprint.columns.1 {
            farthest = farthest.min(pyramid.load(footprint.level, x, y));
        }
    }
    bounds.nearest * (1.0 + OCCLUSION_DEPTH_SLACK) < farthest
}

/// Whether the box's projected longer side is under `pixels` —
/// `occlusionMain`'s small-feature test. A box [`project_box`] refuses is not
/// small.
#[must_use]
pub fn small_feature(
    view_projection: Mat4,
    center: Vec3,
    extent: Vec3,
    target: (u32, u32),
    pixels: f32,
) -> bool {
    project_box(view_projection, center, extent, target).is_some_and(|bounds| {
        (bounds.max[0] - bounds.min[0]).max(bounds.max[1] - bounds.min[1]) < pixels
    })
}

/// What the first occlusion phase is given.
#[derive(Clone, Copy, Debug)]
pub struct EarlyInputs<'a> {
    /// This frame's world → clip, which the small-feature test projects through.
    pub view_projection: Mat4,
    /// The previous frame's, which the occlusion test reprojects through.
    pub previous_view_projection: Mat4,
    /// The previous frame's farthest-depth pyramid, or `None` where there is no
    /// history — which marks nothing.
    pub history: Option<&'a DepthPyramid>,
    /// The target the small-feature test measures in.
    pub target: (u32, u32),
    /// The small-feature threshold in pixels, or `None` where it is off.
    pub small_feature_pixels: Option<f32>,
}

/// The survivor-list entries `cull.slang`'s `occlusionMain` writes, ascending:
/// [`visible_instances`](super::visible_instances)' survivors less the small
/// features, each tagged [`ENTRY_OCCLUDED`] where the previous frame's pyramid
/// hides it.
///
/// A deforming instance — [`GpuInstance::BASE_VERTEX_OVERRIDE`] — is kept
/// untagged, because its source bounds do not describe its vertices.
#[must_use]
pub fn early_entries(
    frustum: &Frustum,
    instances: &[GpuInstance],
    meshes: &[GpuMesh],
    hidden_view: u32,
    inputs: &EarlyInputs<'_>,
) -> Vec<u32> {
    let mut entries = Vec::new();
    for index in super::visible_instances(frustum, instances, meshes, hidden_view) {
        let instance = &instances[index as usize];
        if instance.flags & GpuInstance::BASE_VERTEX_OVERRIDE != 0 {
            entries.push(index);
            continue;
        }
        let mesh = &meshes[instance.mesh as usize];
        let (center, extent) = world_box(instance, mesh);
        if inputs.small_feature_pixels.is_some_and(|pixels| {
            small_feature(
                inputs.view_projection,
                center,
                extent,
                inputs.target,
                pixels,
            )
        }) {
            continue;
        }
        let hidden = inputs.history.is_some_and(|pyramid| {
            occluded(inputs.previous_view_projection, center, extent, pyramid)
        });
        entries.push(if hidden {
            index | ENTRY_OCCLUDED
        } else {
            index
        });
    }
    entries
}

/// What `cull.slang`'s `lateMain` leaves in an entry: an entry the first phase
/// marked, tagged [`ENTRY_RESCUED`] if this frame's pyramid does not hide it.
/// Any other entry is returned as it came.
#[must_use]
pub fn late_entry(
    entry: u32,
    instances: &[GpuInstance],
    meshes: &[GpuMesh],
    view_projection: Mat4,
    pyramid: &DepthPyramid,
) -> u32 {
    if entry & ENTRY_OCCLUDED == 0 {
        return entry;
    }
    let instance = &instances[(entry & crcbl_shaders::cull::ENTRY_INDEX_MASK) as usize];
    let (center, extent) = world_box(instance, &meshes[instance.mesh as usize]);
    if occluded(view_projection, center, extent, pyramid) {
        entry
    } else {
        entry | ENTRY_RESCUED
    }
}

/// Face `f`'s side planes: the first [`FACE_PLANES`] of the frustum of its
/// view-projection, which are its pyramid's four sides.
pub type FacePlanes = [[Vec4; FACE_PLANES]; FACE_COUNT];

/// The six faces' side planes of a point light whose face `f` renders through
/// `view_projections[f]`.
///
/// **Only the sides.** The near and far planes are dropped: a face's near plane
/// excludes a sliver around the light that no face covers but the light's box
/// does, and the far plane is the box's own. So the six pyramids tile every
/// direction out of the light, and a box the light's cull kept reaches at least
/// one of them.
#[must_use]
pub fn face_planes(view_projections: &[Mat4; FACE_COUNT]) -> FacePlanes {
    view_projections.map(|matrix| {
        let planes = Frustum::from_view_projection(matrix).planes;
        [planes[0], planes[1], planes[2], planes[3]]
    })
}

/// The face tags `cull.slang`'s `computeMain` adds to a survivor of a point
/// light's cull under `CULL_FACES`: bit [`ENTRY_FACE_SHIFT`]` + f` for every
/// face `f` whose four side planes the box reaches.
#[must_use]
pub fn face_tags(faces: &FacePlanes, center: Vec3, extent: Vec3) -> u32 {
    let mut tags = 0;
    for (face, planes) in faces.iter().enumerate() {
        let inside = planes.iter().all(|plane| {
            let normal = plane.truncate();
            let radius = normal.abs().dot(extent);
            normal.dot(center) + plane.w >= -radius
        });
        if inside {
            tags |= 1 << (ENTRY_FACE_SHIFT as usize + face);
        }
    }
    tags
}

/// The survivor-list entries a point light's cull writes, ascending:
/// [`visible_instances`](super::visible_instances) against the light's box,
/// each tagged by [`face_tags`] — or with every face, for a deforming instance.
#[must_use]
pub fn face_entries(
    frustum: &Frustum,
    faces: &FacePlanes,
    instances: &[GpuInstance],
    meshes: &[GpuMesh],
) -> Vec<u32> {
    super::visible_instances(frustum, instances, meshes, 0)
        .into_iter()
        .map(|index| {
            let instance = &instances[index as usize];
            if instance.flags & GpuInstance::BASE_VERTEX_OVERRIDE != 0 {
                return index | (((1 << FACE_COUNT) - 1) << ENTRY_FACE_SHIFT);
            }
            let (center, extent) = world_box(instance, &meshes[instance.mesh as usize]);
            index | face_tags(faces, center, extent)
        })
        .collect()
}

#[cfg(test)]
mod tests;
