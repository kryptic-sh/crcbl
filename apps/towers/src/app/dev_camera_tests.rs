//! The dev camera through the running game, headless: the key that switches
//! it, the keys its context takes from the field, the debug panel's rows —
//! and the stage, which it must leave exactly as it would have been.

use crcbl::core::input::KeyCode;
use crcbl::engine::ExitReason;
use crcbl::shell::HeadlessShell;
use crcbl_sample_test::{headless_common, ui_text};

use super::tests::{frames, headless, scripted, tap};
use super::{CAMERA_KEY, Loop, Options};
use crate::dev_camera::{CONTEXT, Mode};

/// Holds `key` down for `count` frames, then lets it go and runs a frame.
fn hold(engine: &mut Loop<HeadlessShell>, key: KeyCode, count: usize) {
    let window = engine.window();
    engine
        .shell_mut()
        .key_press(window, key)
        .expect("the window is live");
    frames(engine, count);
    engine
        .shell_mut()
        .key_release(window, key)
        .expect("the window is live");
    frames(engine, 1);
}

/// **The camera key goes overhead, fly, walk and back to the overhead view
/// exactly**, and while the camera moves its context has the keys it binds:
/// `W` flies, the arrows turn rather than walk the build cursor — and once
/// the overhead view is back, the arrows are the cursor's again.
#[test]
fn the_camera_key_cycles_fly_walk_and_back_to_the_overhead_view() {
    let mut engine = scripted(&headless(400));
    frames(&mut engine, 2);
    let mode = |engine: &Loop<HeadlessShell>| engine.game().dev_camera().mode();
    assert_eq!(mode(&engine), Mode::Overhead);

    tap(&mut engine, CAMERA_KEY);
    assert_eq!(mode(&engine), Mode::Fly);
    assert!(engine.game().actions.is_context_active(CONTEXT));
    let start = engine.game().dev_camera().camera().eye;
    hold(&mut engine, KeyCode::KeyW, 20);
    assert_ne!(
        engine.game().dev_camera().camera().eye,
        start,
        "W did not fly"
    );
    tap(&mut engine, KeyCode::ArrowRight);
    assert_eq!(
        engine.game().selected(),
        0,
        "the arrow moved the build cursor while the camera had it",
    );

    tap(&mut engine, CAMERA_KEY);
    assert_eq!(mode(&engine), Mode::Walk);
    tap(&mut engine, CAMERA_KEY);
    assert_eq!(mode(&engine), Mode::Overhead);
    assert!(!engine.game().actions.is_context_active(CONTEXT));
    assert_eq!(
        engine.game().dev_camera().camera(),
        crate::camera::camera(),
        "the overhead view is not the fixed camera",
    );

    tap(&mut engine, KeyCode::ArrowRight);
    assert_eq!(
        engine.game().selected(),
        1,
        "the arrow is not the build cursor's once the overhead view is back",
    );
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// After one frame: how many ticks the game has run, and the stage's
/// fingerprint — `Game::stage_fingerprint`.
type Seen = (u64, Option<(u64, usize)>);

/// One headless run of `count` frames, the stage's fingerprint after every
/// one — and, when `walk` is set, the dev camera switched to the walk and
/// walked about the field for all of it.
fn fingerprints(count: usize, walk: bool) -> (Vec<Seen>, Loop<HeadlessShell>) {
    let mut engine = scripted(&headless(count as u64 + 8));
    let window = engine.window();
    let mut seen = Vec::with_capacity(count);
    let frame = |engine: &mut Loop<HeadlessShell>, seen: &mut Vec<_>| {
        engine.frame().expect("a frame");
        seen.push((
            engine.game().game().ticks_run(),
            engine.game().game().stage_fingerprint(),
        ));
    };
    for at in 0..count {
        if walk {
            // Into the walk in four frames, dropped at the field's near
            // edge, then down the field, across it and turning.
            let event = match at {
                0 | 2 => Some((CAMERA_KEY, true)),
                1 | 3 => Some((CAMERA_KEY, false)),
                4 => Some((KeyCode::KeyW, true)),
                300 => Some((KeyCode::KeyW, false)),
                301 => Some((KeyCode::KeyA, true)),
                420 => Some((KeyCode::KeyA, false)),
                421 => Some((KeyCode::ArrowLeft, true)),
                480 => Some((KeyCode::ArrowLeft, false)),
                _ => None,
            };
            if let Some((key, pressed)) = event {
                let shell = engine.shell_mut();
                if pressed {
                    shell.key_press(window, key)
                } else {
                    shell.key_release(window, key)
                }
                .expect("the window is live");
            }
        }
        frame(&mut engine, &mut seen);
    }
    (seen, engine)
}

/// **Walking the dev camera leaves the stage exactly as it would have been**:
/// a run that switches to the walk and walks the field for the whole of the
/// first wave hashes as a run that touched nothing, tick for tick, and its
/// stage's physics world holds the same colliders throughout.
///
/// The walk is held to have happened — the walker crossed metres of field —
/// or the two runs agreeing would say nothing.
#[test]
fn walking_the_dev_camera_leaves_the_stage_hash_alone() {
    const FRAMES: usize = 1200;
    let (still, still_engine) = fingerprints(FRAMES, false);
    let (walked, walked_engine) = fingerprints(FRAMES, true);

    let camera = walked_engine.game().dev_camera();
    assert_eq!(camera.mode(), Mode::Walk, "the run never walked");
    // Dropped at the field's near edge, and walked down it and across.
    assert!(
        camera.walker().feet().z < 0.0 && camera.walker().feet().x < -3.0,
        "the walker did not walk across the field: {:?}",
        camera.walker().feet(),
    );
    assert!(
        still
            .iter()
            .any(|(_, print)| print.is_some_and(|(_, colliders)| colliders > 2)),
        "no creep ever entered the stage's world, so the runs compared an empty field",
    );
    for (tick, (a, b)) in still.iter().zip(&walked).enumerate() {
        assert_eq!(a, b, "the stage diverged on frame {tick}");
    }
    assert_eq!(still.len(), walked.len());
    still_engine
        .finish(ExitReason::FrameBudget)
        .expect("teardown");
    walked_engine
        .finish(ExitReason::FrameBudget)
        .expect("teardown");
}

/// **The debug panel says which camera, and in the walk what the walker
/// stands on and what its last move met** — `move_and_slide_into`'s contacts.
#[test]
fn the_debug_panel_shows_the_walkers_ground_and_contacts() {
    let mut common = headless_common(crate::game::DEFAULT_TICK_HZ, 600);
    common.debug_overlay = Some(true);
    let mut engine = scripted(&Options {
        common,
        ..Options::default()
    });
    frames(&mut engine, 2);
    let drawn = |engine: &Loop<HeadlessShell>| ui_text(engine.gpu().draw_list());
    assert!(drawn(&engine).iter().any(|text| text == "overhead"));

    tap(&mut engine, CAMERA_KEY);
    tap(&mut engine, CAMERA_KEY);
    // The drop from the overhead eye's height, which lands it a little inside
    // the field's near edge — then backed into that edge, so every move meets
    // the wall there.
    frames(&mut engine, 3 * crate::game::DEFAULT_TICK_HZ as usize);
    let window = engine.window();
    engine
        .shell_mut()
        .key_press(window, KeyCode::KeyS)
        .expect("the window is live");
    frames(&mut engine, crate::game::DEFAULT_TICK_HZ as usize);
    let text = drawn(&engine);
    for row in [
        "camera",
        "mode",
        "walk",
        "grounded",
        "on the ground",
        "contacts",
        "edge",
    ] {
        assert!(
            text.iter().any(|drawn| drawn == row),
            "the panel has no {row:?}: {text:?}",
        );
    }
    assert!(engine.game().dev_camera().walker().last().hit_wall);
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}
