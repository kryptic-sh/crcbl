use super::*;
use crate::cull::visible_instances;
use crate::light::PointLight;
use crate::shadow;

/// A pass-through projection: world `x` and `y` in `-1..1` are the NDC, and
/// world `z` is the reversed-Z depth itself. Every rectangle and depth below is
/// therefore readable off the numbers that built it.
const FLAT: Mat4 = Mat4::IDENTITY;

/// A depth image of `extent` holding `far` everywhere, with `(x, y, value)`
/// texels written over it.
fn depth_image(extent: (u32, u32), far: f32, texels: &[(u32, u32, f32)]) -> Vec<f32> {
    let mut depth = vec![far; extent.0 as usize * extent.1 as usize];
    for &(x, y, value) in texels {
        depth[y as usize * extent.0 as usize + x as usize] = value;
    }
    depth
}

/// The box whose projection through [`FLAT`] onto `extent` covers pixels
/// `x0..x1` × `y0..y1` exactly, at depth `z`.
fn over_pixels(
    extent: (u32, u32),
    (x0, x1): (f32, f32),
    (y0, y1): (f32, f32),
    z: f32,
) -> (Vec3, Vec3) {
    let ndc_x = |px: f32| px / extent.0 as f32 * 2.0 - 1.0;
    // Pixel rows grow downward and NDC `y` upward.
    let ndc_y = |py: f32| 1.0 - py / extent.1 as f32 * 2.0;
    let min = Vec3::new(ndc_x(x0), ndc_y(y1), z);
    let max = Vec3::new(ndc_x(x1), ndc_y(y0), z);
    ((min + max) * 0.5, (max - min) * 0.5)
}

/// **The trap the plan names, shown on the CPU**: a picket fence of one-pixel
/// posts, near, in front of a far wall, and a box between the two.
///
/// Every block the box's footprint reads has a post in it, so a pyramid holding
/// each block's **nearest** depth reads the posts' depth everywhere and hides the
/// box. Seven pixels in eight show it over the wall, and the farthest pyramid,
/// whose value there is the wall's, keeps it. Both are stated, so the test goes
/// red if the two reductions were swapped or made the same.
#[test]
fn the_nearest_pyramid_hides_what_the_farthest_keeps() {
    let extent = (64, 64);
    // A far wall at depth 0.1 everywhere, and a post one pixel wide at 0.9
    // every eighth column.
    let posts: Vec<(u32, u32, f32)> = (0..64)
        .flat_map(|y| (0..64).step_by(8).map(move |x| (x, y, 0.9)))
        .collect();
    let depth = depth_image(extent, 0.1, &posts);
    let (center, half) = over_pixels(extent, (16.0, 26.0), (16.0, 26.0), 0.5);

    let farthest = DepthPyramid::reduce(&depth, extent, Reduction::Farthest);
    let nearest = DepthPyramid::reduce(&depth, extent, Reduction::Nearest);
    assert!(
        !occluded(FLAT, center, half, &farthest),
        "the box is in front of the wall between the posts, so it is on screen"
    );
    assert!(
        occluded(FLAT, center, half, &nearest),
        "a nearest-depth pyramid reads a post's depth in every block, and hides a box the \
         fence only partly covers — the reason the cull reads its own pyramid"
    );
}

/// A box wholly behind a wall that covers every pixel it projects to is hidden,
/// and the same box moved in front of the wall is not.
#[test]
fn a_box_behind_a_whole_wall_is_hidden_and_one_in_front_is_not() {
    let extent = (64, 48);
    let depth = depth_image(extent, 0.6, &[]);
    let pyramid = DepthPyramid::reduce(&depth, extent, Reduction::Farthest);
    let (center, half) = over_pixels(extent, (10.0, 30.0), (8.0, 20.0), 0.3);
    assert!(occluded(FLAT, center, half, &pyramid));
    let nearer = center + Vec3::Z * 0.5;
    assert!(
        !occluded(FLAT, nearer, half, &pyramid),
        "at depth 0.8 the box is nearer than the wall"
    );
}

/// **One uncovered pixel under the footprint keeps the box**: the far plane's
/// clear is `0.0`, the farthest depth there is, so whatever is behind every
/// other surface can still be seen through that pixel.
#[test]
fn a_single_uncovered_pixel_keeps_what_is_behind_it() {
    let extent = (64, 64);
    let depth = depth_image(extent, 0.9, &[(20, 20, crcbl_hal::depth::CLEAR)]);
    let pyramid = DepthPyramid::reduce(&depth, extent, Reduction::Farthest);
    let (center, half) = over_pixels(extent, (18.0, 23.0), (18.0, 23.0), 0.5);
    assert!(
        !occluded(FLAT, center, half, &pyramid),
        "pixel (20, 20) shows the sky, so the box behind the wall is visible through it"
    );
    // And a box whose footprint misses that pixel by more than the guard band
    // is hidden, which is what says the pixel is what decided it.
    let (clear_of, half) = over_pixels(extent, (40.0, 45.0), (40.0, 45.0), 0.5);
    assert!(occluded(FLAT, clear_of, half, &pyramid));
}

/// **Touching is not hidden.** A box whose nearest depth is the wall's own is
/// kept, because the forward pass's `GreaterOrEqual` keeps a fragment at equal
/// depth; the slack is what keeps rounding from deciding that case.
#[test]
fn a_box_touching_the_occluder_is_kept() {
    let extent = (32, 32);
    let wall = 0.5;
    let pyramid =
        DepthPyramid::reduce(&depth_image(extent, wall, &[]), extent, Reduction::Farthest);
    let (center, half) = over_pixels(extent, (4.0, 12.0), (4.0, 12.0), wall);
    assert!(
        !occluded(FLAT, center, half, &pyramid),
        "at the wall's depth"
    );
    // Inside the slack: a hair behind the wall, still kept.
    let hair = wall * (1.0 - OCCLUSION_DEPTH_SLACK * 0.5);
    let (center, half) = over_pixels(extent, (4.0, 12.0), (4.0, 12.0), hair);
    assert!(!occluded(FLAT, center, half, &pyramid), "inside the slack");
    // Past it: hidden.
    let behind = wall * (1.0 - OCCLUSION_DEPTH_SLACK * 4.0);
    let (center, half) = over_pixels(extent, (4.0, 12.0), (4.0, 12.0), behind);
    assert!(occluded(FLAT, center, half, &pyramid), "past the slack");
}

/// **Every texel a pixel's level index names bounds that pixel's depth from
/// the far side**, at every level of pyramids over odd and even extents.
///
/// This is the property the test above rests on, stated over whole images: the
/// last texel of an odd axis is what covers the row or column the halving
/// floored past, so a pixel is read through the clamped index and never
/// dropped. A reduction that skipped the third tap fails at the odd extents.
#[test]
fn every_pixel_is_bounded_by_the_texel_its_index_clamps_to() {
    let mut state = 0x9e37_79b9_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state % 1000) as f32 / 1000.0
    };
    for extent in [(64, 48), (97, 61), (31, 17), (7, 5), (256, 3)] {
        let depth: Vec<f32> = (0..extent.0 * extent.1).map(|_| next()).collect();
        let pyramid = DepthPyramid::reduce(&depth, extent, Reduction::Farthest);
        assert_eq!(pyramid.levels(), levels_for(extent));
        for level in 1..=pyramid.levels() {
            let (width, height) = level_extent(extent, level);
            for y in 0..extent.1 {
                for x in 0..extent.0 {
                    let tx = (x >> level).min(width - 1);
                    let ty = (y >> level).min(height - 1);
                    let texel = pyramid.level(level)[(ty * width + tx) as usize];
                    let pixel = depth[(y * extent.0 + x) as usize];
                    assert!(
                        texel <= pixel,
                        "level {level} texel ({tx}, {ty}) holds {texel}, nearer than pixel \
                         ({x}, {y})'s {pixel} over {extent:?}"
                    );
                }
            }
        }
    }
}

/// **At most two texels a side**, until the pyramid runs out of levels — the
/// four `Load`s the shader's loop reads for any box short of a quarter screen.
#[test]
fn the_footprint_is_two_texels_a_side_until_the_deepest_level() {
    let target = (256, 192);
    let levels = levels_for(target);
    for (x0, x1) in [(0.0, 1.0), (10.0, 13.0), (100.0, 140.0), (0.0, 200.0)] {
        let bounds = ScreenBounds {
            min: [x0, 50.0],
            max: [x1, 52.0],
            nearest: 0.5,
        };
        let found = footprint(&bounds, target, levels).expect("on screen");
        let wide = found.columns.1 - found.columns.0 + 1;
        let tall = found.rows.1 - found.rows.0 + 1;
        assert!(
            (wide <= 2 && tall <= 2) || found.level == levels,
            "{x0}..{x1} read {wide}×{tall} texels at level {}",
            found.level
        );
        // And the finest level that manages it: one level finer spans more
        // than two texels on some axis.
        if found.level > 1 && found.level < levels {
            let finer = found.level - 1;
            let widened = |low: f32, high: f32, limit: u32| {
                let low = (low.floor() as i32 - OCCLUSION_GUARD_TEXELS).max(0) as u32;
                let high = ((high.floor() as i32 + OCCLUSION_GUARD_TEXELS) as u32).min(limit - 1);
                (high >> finer) - (low >> finer)
            };
            assert!(
                widened(x0, x1, target.0) > 1 || widened(50.0, 52.0, target.1) > 1,
                "level {} was not the finest two-texel level for {x0}..{x1}",
                found.level
            );
        }
    }
    assert_eq!(
        footprint(
            &ScreenBounds {
                min: [300.0, 10.0],
                max: [310.0, 20.0],
                nearest: 0.5,
            },
            target,
            levels
        ),
        None,
        "a rectangle entirely off the target reads nothing"
    );
}

/// **What the pyramid cannot speak for is never hidden**: a box straddling the
/// eye plane, one in front of the near plane, and one projected entirely off
/// screen — all over a pyramid that would hide anything it could see.
#[test]
fn what_cannot_be_projected_or_is_off_screen_is_never_hidden() {
    let extent = (64, 64);
    let pyramid = DepthPyramid::reduce(&depth_image(extent, 1.0, &[]), extent, Reduction::Farthest);
    let camera = crate::camera::Camera::default();
    let view_projection = camera.view_projection(1.0);
    // Around the eye.
    assert!(!occluded(
        view_projection,
        camera.eye,
        Vec3::splat(0.5),
        &pyramid
    ));
    // Far off to the side, in front of the camera.
    assert!(!occluded(
        view_projection,
        Vec3::new(500.0, 0.0, -10.0),
        Vec3::splat(0.5),
        &pyramid
    ));
    // Through the pass-through projection: depth past 1 is in front of the near
    // plane.
    let (center, half) = over_pixels(extent, (4.0, 8.0), (4.0, 8.0), 1.5);
    assert!(!occluded(FLAT, center, half, &pyramid));
    assert_eq!(project_box(FLAT, center, half, extent), None);
}

/// The small-feature test measures the projected box's **longer** side.
#[test]
fn a_small_feature_is_small_on_its_longer_side() {
    let extent = (64, 64);
    let (center, half) = over_pixels(extent, (10.0, 12.0), (10.0, 11.0), 0.5);
    assert!(
        small_feature(FLAT, center, half, extent, 3.0),
        "two pixels wide"
    );
    assert!(
        !small_feature(FLAT, center, half, extent, 2.0),
        "not under two"
    );
    let (center, half) = over_pixels(extent, (10.0, 11.0), (10.0, 30.0), 0.5);
    assert!(
        !small_feature(FLAT, center, half, extent, 3.0),
        "one pixel wide and twenty tall is not small"
    );
}

/// A point light's six face pyramids **tile its box**: every box the light's
/// cull keeps reaches at least one face, a box off one axis reaches that axis's
/// face and not its opposite, and one around the light reaches all six.
#[test]
fn a_point_lights_faces_tile_its_box() {
    let light = PointLight {
        position: Vec3::new(1.0, 2.0, -3.0),
        radius: 4.0,
        color: Vec3::ONE,
        fill: false,
    };
    let matrices: [Mat4; shadow::POINT_FACES] =
        core::array::from_fn(|face| shadow::point_matrix(&light, face));
    let faces = face_planes(&matrices);
    let box_cull = shadow::point_frustum(&light);
    let tags = |center: Vec3, extent: Vec3| face_tags(&faces, center, extent) >> ENTRY_FACE_SHIFT;

    let beside = |axis: Vec3| light.position + axis * 3.0;
    let small = Vec3::splat(0.25);
    // `face_axis`' order: +X, -X, +Y, -Y, +Z, -Z.
    for face in 0..shadow::POINT_FACES {
        let axis = shadow::face_axis(face);
        let found = tags(beside(axis), small);
        assert_ne!(
            found & (1 << face),
            0,
            "a box along face {face}'s axis reaches it"
        );
        let opposite = face ^ 1;
        assert_eq!(
            found & (1 << opposite),
            0,
            "and not the face looking the other way"
        );
    }
    assert_eq!(
        tags(light.position, small),
        0b11_1111,
        "around the light, every face"
    );

    // Every box the light's own cull keeps reaches a face: a grid of small
    // boxes through the whole box and a little past it.
    let mut kept = 0;
    for x in -5..=5 {
        for y in -5..=5 {
            for z in -5..=5 {
                let center = light.position + Vec3::new(x as f32, y as f32, z as f32) * 0.9;
                let bounds = crate::cull::Aabb {
                    min: center - small,
                    max: center + small,
                };
                if box_cull.intersects(&bounds) {
                    kept += 1;
                    assert_ne!(
                        tags(center, small),
                        0,
                        "a box the light keeps at {center:?} reaches no face"
                    );
                }
            }
        }
    }
    assert!(
        kept > 500,
        "the grid put {kept} boxes inside the light, which is the sample"
    );
}

/// **The early phase marks, and the late phase rescues or keeps the mark**, on
/// a row of instances behind a wall and one in front of it.
#[test]
fn the_early_phase_marks_and_the_late_phase_rescues() {
    let extent = (64, 64);
    let camera = crate::camera::Camera::default();
    let view_projection = camera.view_projection(1.0);
    let frustum = Frustum::from_view_projection(view_projection);
    let mesh = GpuMesh {
        index_count: 36,
        bounds_min: [-0.05; 3],
        bounds_max: [0.05; 3],
        ..GpuMesh::default()
    };
    let at = |translation: Vec3| GpuInstance {
        transform: Mat4::from_translation(translation).to_cols_array(),
        flags: GpuInstance::LIVE,
        ..GpuInstance::default()
    };
    // The default camera is two metres back along +Z looking at the origin.
    let instances = [at(Vec3::new(0.0, 0.0, -1.0)), at(Vec3::new(0.0, 0.0, 1.0))];
    let meshes = [mesh];
    // A wall at the depth a surface half a metre in front of the origin has.
    let wall = camera
        .depth_of(Vec3::new(0.0, 0.0, 0.5), 1.0)
        .expect("in front of the eye");
    let history =
        DepthPyramid::reduce(&depth_image(extent, wall, &[]), extent, Reduction::Farthest);
    let inputs = EarlyInputs {
        view_projection,
        previous_view_projection: view_projection,
        history: Some(&history),
        target: extent,
        small_feature_pixels: None,
    };
    let early = early_entries(&frustum, &instances, &meshes, 0, &inputs);
    assert_eq!(
        early,
        vec![ENTRY_OCCLUDED, 1],
        "the instance behind the wall is marked, the one in front of it is not"
    );
    assert_eq!(
        visible_instances(&frustum, &instances, &meshes, 0),
        vec![0, 1],
        "and both are still survivors"
    );

    // This frame's early depth holds the wall: the mark stands.
    assert_eq!(
        late_entry(early[0], &instances, &meshes, view_projection, &history),
        ENTRY_OCCLUDED
    );
    // This frame's early depth holds nothing there — the wall moved: rescued.
    let empty = DepthPyramid::reduce(
        &depth_image(extent, crcbl_hal::depth::CLEAR, &[]),
        extent,
        Reduction::Farthest,
    );
    assert_eq!(
        late_entry(early[0], &instances, &meshes, view_projection, &empty),
        ENTRY_OCCLUDED | ENTRY_RESCUED
    );
    // An unmarked entry is not the late phase's business.
    assert_eq!(
        late_entry(early[1], &instances, &meshes, view_projection, &empty),
        1
    );

    // No history marks nothing.
    let without = EarlyInputs {
        history: None,
        ..inputs
    };
    assert_eq!(
        early_entries(&frustum, &instances, &meshes, 0, &without),
        vec![0, 1]
    );
    // And a threshold drops what is under it before anything is marked.
    let small = EarlyInputs {
        small_feature_pixels: Some(64.0),
        ..inputs
    };
    assert!(early_entries(&frustum, &instances, &meshes, 0, &small).is_empty());
}
