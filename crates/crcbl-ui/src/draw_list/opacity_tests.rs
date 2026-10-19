//! [`DrawList::push_opacity`] and [`DrawList::pop_opacity`]: the groups they
//! record, and the runs the expansion turns them into.

use super::*;

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

/// A unit-ish square at `x`, so each command's quad is told apart by where it
/// is.
fn square(list: &mut DrawList, x: f32) {
    list.rect(Vec2::new(x, 0.0), Vec2::new(x + 10.0, 10.0), RED);
}

fn group(commands: Range<usize>, opacity: f32) -> OpacityGroup {
    OpacityGroup { commands, opacity }
}

/// **A push and its pop enclose the commands between them**, and nothing
/// before or after.
#[test]
fn a_group_is_the_commands_between_its_push_and_pop() {
    let mut list = DrawList::new();
    square(&mut list, 0.0);
    list.push_opacity(0.5);
    square(&mut list, 20.0);
    square(&mut list, 40.0);
    list.pop_opacity().expect("pushed above");
    square(&mut list, 60.0);
    assert_eq!(list.opacity_groups(), &[group(1..3, 0.5)]);
}

/// **Nested groups lie inside their parent and come after it**, siblings in
/// push order.
#[test]
fn nested_groups_lie_inside_their_parent_in_push_order() {
    let mut list = DrawList::new();
    list.push_opacity(0.8);
    square(&mut list, 0.0);
    list.push_opacity(0.5);
    square(&mut list, 20.0);
    list.pop_opacity().expect("pushed above");
    list.push_opacity(0.25);
    square(&mut list, 40.0);
    list.pop_opacity().expect("pushed above");
    square(&mut list, 60.0);
    list.pop_opacity().expect("pushed above");
    assert_eq!(
        list.opacity_groups(),
        &[group(0..4, 0.8), group(1..2, 0.5), group(2..3, 0.25)]
    );
}

/// **An opacity of one records nothing, and still wants its pop**: the pop
/// after it closes it rather than the group beneath.
#[test]
fn an_opacity_of_one_records_nothing_but_still_pairs_with_its_pop() {
    let mut list = DrawList::new();
    list.push_opacity(0.5);
    list.push_opacity(1.0);
    square(&mut list, 0.0);
    list.pop_opacity().expect("the push at one");
    square(&mut list, 20.0);
    list.pop_opacity().expect("the push at a half");
    assert_eq!(list.opacity_groups(), &[group(0..2, 0.5)]);
    assert_eq!(list.pop_opacity(), Err(OpacityUnderflow));
}

/// **The opacity is clamped into `0..=1`, and NaN is one**: above one is no
/// group at all, below zero is a group at zero.
#[test]
fn the_opacity_is_clamped_and_nan_is_one() {
    for (pushed, recorded) in [
        (1.5, None),
        (f32::INFINITY, None),
        (f32::NAN, None),
        (-0.5, Some(0.0)),
        (f32::NEG_INFINITY, Some(0.0)),
        (0.3, Some(0.3)),
    ] {
        let mut list = DrawList::new();
        list.push_opacity(pushed);
        square(&mut list, 0.0);
        list.pop_opacity().expect("pushed above");
        let want: Vec<OpacityGroup> = recorded.map(|o| group(0..1, o)).into_iter().collect();
        assert_eq!(list.opacity_groups(), want.as_slice(), "pushed {pushed}");
    }
}

/// **A pop with no push is an error and changes nothing**, as
/// [`DrawList::pop_clip`]'s is.
#[test]
fn a_pop_with_no_push_is_an_underflow() {
    let mut list = DrawList::new();
    assert_eq!(list.pop_opacity(), Err(OpacityUnderflow));
    list.push_opacity(0.5);
    square(&mut list, 0.0);
    list.pop_opacity().expect("pushed above");
    assert_eq!(list.pop_opacity(), Err(OpacityUnderflow));
    assert_eq!(list.opacity_groups(), &[group(0..1, 0.5)]);
    assert_eq!(
        OpacityUnderflow.to_string(),
        "pop_opacity was called with no opacity group pushed"
    );
}

/// **A group left open covers everything pushed after it**, as a clip left
/// pushed does — until the overlay cut, which closes it, and a clear, which
/// drops it.
#[test]
fn an_open_group_runs_to_the_cut_and_a_clear_drops_it() {
    let mut list = DrawList::new();
    list.push_opacity(0.5);
    square(&mut list, 0.0);
    square(&mut list, 20.0);
    assert_eq!(list.opacity_groups(), &[group(0..2, 0.5)]);
    list.begin_overlay();
    square(&mut list, 40.0);
    assert_eq!(list.opacity_groups(), &[group(0..2, 0.5)]);
    assert_eq!(
        list.pop_opacity(),
        Err(OpacityUnderflow),
        "the cut closed it"
    );

    list.push_opacity(0.5);
    list.clear();
    assert!(list.opacity_groups().is_empty());
    assert_eq!(
        list.pop_opacity(),
        Err(OpacityUnderflow),
        "the clear closed it"
    );
}

/// **The expansion turns each group into the indices its commands drew**,
/// with the rectangle they can cover; a group at one draws no run at all.
#[test]
fn a_group_expands_to_its_commands_indices_and_bounds() {
    let mut list = DrawList::new();
    square(&mut list, 0.0);
    list.push_opacity(0.5);
    square(&mut list, 20.0);
    list.push_opacity(1.0);
    square(&mut list, 40.0);
    list.pop_opacity().expect("pushed above");
    list.pop_opacity().expect("pushed above");
    square(&mut list, 60.0);

    let triangles = list.to_triangles_split(None, None, 1.0);
    assert_eq!(triangles.indices.len(), 24, "every square still draws");
    assert_eq!(
        triangles.groups,
        [OpacityRun {
            indices: 6..18,
            opacity: 0.5,
            bounds: ClipRect {
                min: Vec2::new(20.0, 0.0),
                max: Vec2::new(50.0, 10.0),
            },
        }]
    );
}

/// **A group's bounds stop at each command's clip**: a square pushed under a
/// narrower clip can cover no more than the clip.
#[test]
fn a_groups_bounds_are_clamped_to_its_commands_clips() {
    let mut list = DrawList::new();
    list.push_opacity(0.5);
    list.push_clip(Vec2::new(2.0, 3.0), Vec2::new(6.0, 100.0));
    square(&mut list, 0.0);
    list.pop_clip().expect("pushed above");
    list.pop_opacity().expect("pushed above");
    let triangles = list.to_triangles_split(None, None, 1.0);
    assert_eq!(
        triangles.groups[0].bounds,
        ClipRect {
            min: Vec2::new(2.0, 3.0),
            max: Vec2::new(6.0, 10.0),
        }
    );
}

/// **A group at zero draws nothing, nested groups and all**, and is no run;
/// the commands either side still draw.
#[test]
fn a_group_at_zero_leaves_its_commands_out_of_the_triangles() {
    let mut list = DrawList::new();
    square(&mut list, 0.0);
    list.push_opacity(0.0);
    square(&mut list, 20.0);
    list.push_opacity(0.5);
    square(&mut list, 40.0);
    list.pop_opacity().expect("pushed above");
    list.pop_opacity().expect("pushed above");
    square(&mut list, 60.0);
    assert_eq!(list.len(), 4, "the hidden commands stay in the list");

    let triangles = list.to_triangles_split(None, None, 1.0);
    assert_eq!(triangles.indices.len(), 12);
    let xs: Vec<f32> = triangles
        .vertices
        .iter()
        .map(|vertex| vertex.pos.x)
        .collect();
    assert_eq!(xs, [0.0, 10.0, 10.0, 0.0, 60.0, 70.0, 70.0, 60.0]);
    assert!(triangles.groups.is_empty());
}

/// **A group that drew nothing is no run**: an empty group, and one of only
/// commands that expand to no triangles.
#[test]
fn a_group_that_drew_nothing_is_no_run() {
    let mut list = DrawList::new();
    list.push_opacity(0.5);
    list.pop_opacity().expect("pushed above");
    list.push_opacity(0.5);
    list.text(Vec2::ZERO, "no atlas, no glyphs", RED, 13.0);
    list.pop_opacity().expect("pushed above");
    assert_eq!(list.opacity_groups().len(), 2);
    assert!(list.to_triangles_split(None, None, 1.0).groups.is_empty());
}

/// **Nested runs keep their parent's order and lie inside it**, on either
/// side of the overlay cut.
#[test]
fn nested_runs_lie_inside_their_parent_and_respect_the_cut() {
    let mut list = DrawList::new();
    list.push_opacity(0.8);
    square(&mut list, 0.0);
    list.push_opacity(0.5);
    square(&mut list, 20.0);
    list.begin_overlay();
    list.push_opacity(0.25);
    square(&mut list, 40.0);
    list.pop_opacity().expect("pushed above");

    let triangles = list.to_triangles_split(None, None, 1.0);
    assert_eq!(triangles.overlay, 12);
    let runs: Vec<(Range<u32>, f32)> = triangles
        .groups
        .iter()
        .map(|run| (run.indices.clone(), run.opacity))
        .collect();
    assert_eq!(runs, [(0..12, 0.8), (6..12, 0.5), (12..18, 0.25)]);
}

/// **The layer quad covers the bounds snapped outward to whole pixels and kept
/// inside the viewport**, its UVs the layer's own at each corner and its
/// colour the opacity in every lane.
#[test]
fn the_layer_quad_covers_the_snapped_bounds_inside_the_viewport() {
    let run = OpacityRun {
        indices: 0..6,
        opacity: 0.25,
        bounds: ClipRect {
            min: Vec2::new(-3.5, 10.25),
            max: Vec2::new(40.5, 300.0),
        },
    };
    let quad = run.layer_quad((100, 200));
    let corners: Vec<(Vec2, Vec2)> = quad.iter().map(|vertex| (vertex.pos, vertex.uv)).collect();
    assert_eq!(
        corners,
        [
            (Vec2::new(0.0, 200.0), Vec2::new(0.0, 1.0)),
            (Vec2::new(41.0, 200.0), Vec2::new(0.41, 1.0)),
            (Vec2::new(41.0, 10.0), Vec2::new(0.41, 0.05)),
            (Vec2::new(0.0, 10.0), Vec2::new(0.0, 0.05)),
        ]
    );
    for vertex in quad {
        assert_eq!(vertex.color, [0.25; 4]);
        assert_eq!(vertex.primitive(), Some(Primitive::Texture));
        assert_eq!(vertex.clip, ClipRect::NONE.lane());
    }
}
