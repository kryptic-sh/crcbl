use crcbl::ui::draw_list::DrawCommand;

use super::*;

/// The default window's size.
const EXTENT: (u32, u32) = (960, 720);

/// A field with `creeps` on it, live in order.
fn field(creeps: &[CreepView]) -> RenderState {
    let mut state = RenderState::default();
    state.creeps[..creeps.len()].copy_from_slice(creeps);
    state.creeps_alive = creeps.len();
    state
}

/// A creep at `(x, z)` on the lane, with `health` left.
fn creep(x: f64, z: f64, health: f32, slowed: bool) -> CreepView {
    CreepView {
        centre: DVec3::new(x, CREEP_RADIUS, z),
        health,
        slowed,
        ..CreepView::default()
    }
}

/// **A bar for every live creep, centred on the point [`LIFT_M`] over its
/// top**, projected through the frame's own camera — filled to its health,
/// tinted when it is held, and drawn above the creep on the screen.
#[test]
fn every_live_creep_has_a_bar_over_it_filled_to_its_health() {
    let camera = crate::camera::camera();
    let creeps = [creep(-10.0, 8.0, 1.0, false), creep(4.0, -6.0, 0.25, true)];
    let bars = bars(&field(&creeps), &camera, EXTENT);
    assert_eq!(bars.len(), creeps.len());
    for (bar, creep) in bars.iter().zip(&creeps) {
        let top = creep.centre + DVec3::Y * CREEP_RADIUS;
        let over = top + DVec3::Y * LIFT_M;
        assert_eq!(Some(bar.at), camera.pixel_of(over.as_vec3(), EXTENT));
        let below = camera
            .pixel_of(top.as_vec3(), EXTENT)
            .expect("the creep is in front of the camera");
        assert!(bar.at.y < below.y, "the bar is not above its creep");
        assert_eq!(bar.fill, creep.health);
        assert_eq!(bar.slowed, creep.slowed);
    }
    assert_eq!(bars[0].colour(), FILL);
    assert_eq!(bars[1].colour(), SLOWED_FILL);
}

/// **Bars come and go with the creeps**: none on an empty field, one per
/// live creep and none for the parked slots past them.
#[test]
fn bars_come_and_go_with_the_creeps() {
    let camera = crate::camera::camera();
    assert!(bars(&field(&[]), &camera, EXTENT).is_empty());
    let mut state = field(&[creep(0.0, 8.0, 1.0, false); 3]);
    assert_eq!(bars(&state, &camera, EXTENT).len(), 3);
    state.creeps_alive = 1;
    assert_eq!(bars(&state, &camera, EXTENT).len(), 1);
}

/// **A creep behind the eye has no bar**, and a frame with no pixels has
/// none at all.
#[test]
fn a_creep_behind_the_eye_or_a_frame_with_no_pixels_has_no_bar() {
    let camera = crate::camera::camera();
    // Over and behind the eye, where the overhead camera faces away from.
    let behind = CreepView {
        centre: (camera.eye + (camera.eye - camera.target)).as_dvec3(),
        ..creep(0.0, 0.0, 1.0, false)
    };
    let state = field(&[behind, creep(0.0, 8.0, 1.0, false)]);
    assert_eq!(bars(&state, &camera, EXTENT).len(), 1);
    assert!(bars(&state, &camera, (0, 0)).is_empty());
}

/// **The fill is as wide as the health, from the bar's left edge**, over a
/// track the bar's full width — and a creep at no health draws the track
/// alone.
#[test]
fn the_fill_is_drawn_over_the_track_as_wide_as_the_health() {
    let at = Vec2::new(100.0, 50.0);
    let rects = |fill: f32| {
        let mut list = DrawList::new();
        draw(
            &mut list,
            &[Bar {
                at,
                fill,
                slowed: false,
            }],
        );
        list.commands()
            .iter()
            .map(|command| match command {
                DrawCommand::Rect { min, max, color } => (*min, *max, *color),
                other => panic!("a bar drew {other:?}"),
            })
            .collect::<Vec<_>>()
    };
    let half = rects(0.5);
    assert_eq!(half.len(), 2);
    let (track_min, track_max, track) = half[0];
    assert_eq!(track, TRACK);
    assert_eq!(track_max - track_min, Vec2::new(WIDTH_PX, HEIGHT_PX));
    assert_eq!((track_min + track_max) * 0.5, at);
    let (fill_min, fill_max, fill) = half[1];
    assert_eq!(fill, FILL);
    assert_eq!(fill_min, track_min);
    assert_eq!(
        fill_max,
        Vec2::new(track_min.x + 0.5 * WIDTH_PX, track_max.y)
    );

    assert_eq!(rects(1.0)[1].1, track_max);
    assert_eq!(rects(0.0).len(), 1, "an empty bar drew a fill");
}
