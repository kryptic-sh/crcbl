//! [`DrawList::texture`]: a rectangle of a renderer-owned texture, and the
//! [`TextureRun`]s its expansion reports.

use super::*;

const WHITE: [f32; 4] = [1.0; 4];
const WHOLE: (Vec2, Vec2) = (Vec2::ZERO, Vec2::ONE);

fn view(index: u32) -> TextureId {
    TextureId::new(index)
}

/// **A texture rectangle is one quad carrying its UVs, its tint, its clip and
/// its texture**, and the expansion names its indices as a run of that texture.
#[test]
fn a_texture_rectangle_is_a_quad_of_its_own_texture_run() {
    let mut list = DrawList::new();
    list.rect(Vec2::ZERO, Vec2::splat(4.0), WHITE);
    list.push_clip(Vec2::new(1.0, 2.0), Vec2::new(30.0, 40.0));
    let uv = (Vec2::new(0.25, 0.5), Vec2::new(0.75, 1.0));
    list.texture(
        Vec2::new(10.0, 20.0),
        Vec2::new(50.0, 60.0),
        view(7),
        uv,
        [0.5, 1.0, 1.0, 1.0],
    );
    list.pop_clip().expect("pushed above");

    let triangles = list.to_triangles_split(None, None, 1.0);
    assert_eq!(
        triangles.textures,
        [TextureRun {
            texture: view(7),
            indices: 6..12,
        }],
        "the rect's six indices sample nothing, the texture's six sample view 7",
    );
    let quad: Vec<&Vertex2d> = triangles.indices[6..12]
        .iter()
        .map(|&index| &triangles.vertices[index as usize])
        .collect();
    for vertex in &quad {
        assert_eq!(vertex.primitive(), Some(Primitive::Texture));
        assert_eq!(
            vertex.shape[0], 7.0,
            "the texture rides the first shape lane"
        );
        assert_eq!(vertex.color, [0.5, 1.0, 1.0, 1.0]);
        assert_eq!(vertex.clip, [1.0, 2.0, 30.0, 40.0]);
    }
    let corner = |pos: Vec2| {
        quad.iter()
            .find(|vertex| vertex.pos == pos)
            .unwrap_or_else(|| panic!("no vertex at {pos:?}"))
            .uv
    };
    assert_eq!(
        corner(Vec2::new(10.0, 20.0)),
        uv.0,
        "uv_min is drawn at min"
    );
    assert_eq!(
        corner(Vec2::new(50.0, 60.0)),
        uv.1,
        "uv_max is drawn at max"
    );
}

/// **A run is exactly one bind's worth**: consecutive rectangles of one
/// texture share a run, a different texture starts one, anything drawn in
/// between ends one, and the overlay cut ends one.
#[test]
fn runs_merge_only_across_consecutive_rectangles_of_one_texture() {
    let mut list = DrawList::new();
    let at = |x: f32| (Vec2::new(x, 0.0), Vec2::new(x + 1.0, 1.0));
    let (min, max) = at(0.0);
    list.texture(min, max, view(1), WHOLE, WHITE); // 0..6
    let (min, max) = at(1.0);
    list.texture(min, max, view(1), WHOLE, WHITE); // 6..12, merged
    let (min, max) = at(2.0);
    list.texture(min, max, view(2), WHOLE, WHITE); // 12..18, another texture
    list.rect(Vec2::ZERO, Vec2::ONE, WHITE); // 18..24, no texture
    let (min, max) = at(3.0);
    list.texture(min, max, view(2), WHOLE, WHITE); // 24..30, after a rect
    list.begin_overlay();
    let (min, max) = at(4.0);
    list.texture(min, max, view(2), WHOLE, WHITE); // 30..36, above the cut

    let triangles = list.to_triangles_split(None, None, 1.0);
    assert_eq!(triangles.overlay, 30);
    let runs: Vec<(u32, Range<u32>)> = triangles
        .textures
        .iter()
        .map(|run| (run.texture.index(), run.indices.clone()))
        .collect();
    assert_eq!(
        runs,
        [(1, 0..12), (2, 12..18), (2, 24..30), (2, 30..36)],
        "every run must be one texture, contiguous, and on one side of the cut",
    );
}

/// **The rectangle scales like every other push; the UVs and the texture do
/// not**, so a view drawn into a pane laid out in logical pixels lands on the
/// pane's window pixels.
#[test]
fn a_texture_rectangle_honours_the_lists_scale() {
    let mut list = DrawList::new();
    list.set_scale(2.0);
    let uv = (Vec2::new(0.0, 0.25), Vec2::new(1.0, 0.75));
    list.texture(
        Vec2::new(3.0, 4.0),
        Vec2::new(13.0, 24.0),
        view(3),
        uv,
        WHITE,
    );
    let [
        DrawCommand::Texture {
            texture,
            min,
            max,
            uv_min,
            uv_max,
            ..
        },
    ] = list.commands()
    else {
        panic!("one texture command: {:?}", list.commands());
    };
    assert_eq!(*texture, view(3));
    assert_eq!((*min, *max), (Vec2::new(6.0, 8.0), Vec2::new(26.0, 48.0)));
    assert_eq!((*uv_min, *uv_max), uv);
}
