//! UI scale through the host loop, including layout, drawing and input.

use super::*;

/// A loop whose stack says `ui_scale = multiplier`, on a window of `size`
/// logical units at a scale factor of one, with its swapchain settled and
/// its start menu put away.
fn at_ui_scale(multiplier: f32, size: crcbl_shell::LogicalSize) -> Hosted {
    let stack = a_stack();
    crate::settings::set_ui_scale(&mut stack.stack_mut(), multiplier)
        .expect("memory storage takes every key");
    let desc = crcbl_shell::WindowDesc {
        size,
        ..crcbl_shell::WindowDesc::default()
    };
    let mut engine = hosted_with(
        &desc,
        None,
        FakeGame {
            settings: Some(stack),
            ..FakeGame::default()
        },
    );
    serve(&mut engine);
    engine
}

/// The fixture's window, and the window of half its size that the same
/// loop at a UI scale of two lays its own UI out over.
const FULL: crcbl_shell::LogicalSize = crcbl_shell::LogicalSize::new(1280.0, 720.0);
/// See [`FULL`].
const HALF: crcbl_shell::LogicalSize = crcbl_shell::LogicalSize::new(640.0, 360.0);

/// Each command's `Debug` text, which is how a list is compared here:
/// `DrawCommand` has no `PartialEq`.
fn spelled(commands: &[crcbl_ui::draw_list::DrawCommand]) -> Vec<String> {
    commands
        .iter()
        .map(|command| format!("{command:?}"))
        .collect()
}

/// `engine`'s menu drawn from `layout` into a list of its own at `scale`,
/// [`spelled`].
fn menu_drawn(engine: &Hosted, layout: &crcbl_ui::menu::MenuLayout, scale: f32) -> Vec<String> {
    let mut list = crcbl_ui::draw_list::DrawList::new();
    list.set_scale(scale);
    engine.menus.current().expect("a menu is on screen").render(
        &mut list,
        layout,
        engine.gpu().menu_skin(),
    );
    spelled(list.commands())
}

/// **The loop's own menu is laid out over the window divided by its UI
/// scale and recorded at that scale**: at two it is the menu a window of
/// half the size lays out, every command recorded twice the size, so it
/// fills the same share of the window.
#[test]
fn the_loops_own_menu_is_laid_out_logically_and_recorded_at_its_scale() {
    let mut scaled = at_ui_scale(2.0, FULL);
    let mut half = at_ui_scale(1.0, HALF);
    pause_key(&mut scaled);
    pause_key(&mut half);
    assert_eq!(scaled.ui_scale(), 2.0);
    assert_eq!(half.ui_scale(), 1.0);
    assert_eq!(scaled.gpu().extent(), (1280, 720));
    assert_eq!(half.gpu().extent(), (640, 360));

    let layout = scaled.menu_layout().expect("the pause menu");
    assert_eq!(
        Some(&layout),
        half.menu_layout().as_ref(),
        "the menu was not laid out over the logical extent",
    );
    let recorded = spelled(scaled.gpu.draw_list.overlay_commands());
    let at_two = menu_drawn(&scaled, &layout, 2.0);
    assert!(
        recorded.starts_with(&at_two),
        "the menu was not recorded at the loop's UI scale",
    );
    let at_one = menu_drawn(&half, &layout, 1.0);
    assert!(spelled(half.gpu.draw_list.overlay_commands()).starts_with(&at_one));
    assert_ne!(at_two, at_one, "the scale changed nothing to compare");
    // The game's HUD is the game's: it set no scale, so it is drawn at one
    // under a loop whose own UI is at two.
    assert_eq!(
        spelled(scaled.gpu.draw_list.base_commands()),
        spelled(half.gpu.draw_list.base_commands()),
        "the loop's scale reached the game's draw",
    );

    // Known rectangles: the scrim covers the whole window at either
    // scale, and RESUME's nine-slice starts at its logical corner on the
    // half-size window and at twice that on the full one.
    assert!(at_one[0].contains("min: Vec2(0.0, 0.0), max: Vec2(640.0, 360.0)"));
    assert!(
        recorded[0].contains("min: Vec2(0.0, 0.0), max: Vec2(1280.0, 720.0)"),
        "the scrim does not cover the window: {}",
        recorded[0],
    );
    let item = layout.items()[0];
    let corners = |commands: &[String], min, max| {
        commands
            .iter()
            .any(|command| command.contains(&format!("min: {min:?}")))
            && commands
                .iter()
                .any(|command| command.contains(&format!("max: {max:?}")))
    };
    assert!(corners(&at_one, item.min, item.max));
    assert!(
        corners(&recorded, item.min * 2.0, item.max * 2.0),
        "RESUME's button is not at twice its logical rectangle",
    );
}

/// **A pointer on the loop's own menu is hit-tested in its logical
/// pixels**: a click at twice RESUME's logical centre fires it at a UI
/// scale of two, as a click at the centre does at one — and a second
/// finger's contact, which reaches the menu on its own path, likewise.
#[test]
fn a_click_on_the_loops_menu_is_mapped_through_its_scale() {
    let mut half = at_ui_scale(1.0, HALF);
    pause_key(&mut half);
    let centre = menu_button(&half);
    click(&mut half, centre);
    assert!(!half.is_paused(), "RESUME at one did not resume");

    let mut scaled = at_ui_scale(2.0, FULL);
    pause_key(&mut scaled);
    assert_eq!(menu_button(&scaled), centre, "the logical layouts differ");
    click(&mut scaled, centre * 2.0);
    assert!(
        !scaled.is_paused(),
        "a click on RESUME, in window pixels, missed it at a UI scale of two",
    );

    use crcbl_core::input::TouchPhase;
    let mut scaled = at_ui_scale(2.0, FULL);
    primary_finger(
        &mut scaled,
        1,
        TouchPhase::Began,
        glam::Vec2::new(40.0, 700.0),
    );
    pause_key(&mut scaled);
    finger(&mut scaled, 2, TouchPhase::Began, centre * 2.0);
    finger(&mut scaled, 2, TouchPhase::Ended, centre * 2.0);
    assert!(
        !scaled.is_paused(),
        "a second finger on RESUME missed it at a UI scale of two",
    );
}

/// **The console's button and its panel are hit-tested in the loop's
/// logical pixels too**: at a UI scale of two, a finger at twice the
/// button's logical centre opens the console, and taps at twice the
/// on-screen keyboard's logical keys spell a line — and the button is
/// drawn where it was tapped.
#[test]
fn the_console_and_its_button_are_mapped_through_the_loops_scale() {
    let mut engine = at_ui_scale(2.0, FULL);
    let logical = engine.ui_extent();
    assert_eq!(logical, (640, 360));
    glass(&mut engine, ConsoleButton::centre(logical) * 2.0);
    assert!(
        engine.console().is_open(),
        "a tap on the console button, in window pixels, missed it",
    );
    // Drawn last, and exactly the button recorded at two — whose first
    // rectangle is under the finger that tapped it.
    let mut button = crcbl_ui::draw_list::DrawList::new();
    button.set_scale(2.0);
    engine
        .console_button
        .render(&mut button, engine.gpu().atlas());
    let button = spelled(button.commands());
    let recorded = spelled(engine.gpu.draw_list.overlay_commands());
    assert!(
        recorded.ends_with(&button),
        "the console button was not recorded at the loop's UI scale",
    );
    let centre = ConsoleButton::centre(logical) * 2.0;
    let crcbl_ui::draw_list::DrawCommand::Rect { min, max, .. } =
        engine.gpu.draw_list.overlay_commands()[recorded.len() - button.len()]
    else {
        panic!("the button starts with its fill: {}", button[0]);
    };
    assert!(
        min.cmple(centre).all() && centre.cmple(max).all(),
        "the console button is not drawn where it was tapped: {min}..{max} misses {centre}",
    );

    for character in ['e', 'c', 'h', 'o'] {
        let at = key_at(&engine, crcbl_ui::console::KeyCap::Type(character)) * 2.0;
        glass(&mut engine, at);
    }
    step(&mut engine);
    assert_eq!(
        engine.console().panel().line(),
        "echo",
        "taps on the keyboard, in window pixels, missed its keys",
    );
    assert_eq!(presses(&engine), 0, "a tap on the console reached the game");
}

/// **At a UI scale of one the loop's own UI is recorded as it always
/// was**: the menu laid out over the swapchain's own extent, in a list at
/// one — the path the loop drew it through before it had a scale.
#[test]
fn at_a_scale_of_one_the_loops_ui_is_recorded_as_before() {
    let mut engine = hosted(None);
    serve(&mut engine);
    pause_key(&mut engine);
    assert_eq!(engine.ui_scale(), 1.0);
    let layout = engine
        .menus
        .current()
        .expect("the pause menu")
        .layout(engine.gpu().extent(), engine.gpu().atlas());
    assert_eq!(engine.menu_layout(), Some(layout.clone()));
    let before = menu_drawn(&engine, &layout, 1.0);
    assert!(
        spelled(engine.gpu.draw_list.overlay_commands()).starts_with(&before),
        "the menu at one is not what it was",
    );
    assert_eq!(engine.gpu.draw_list.scale(), 1.0);
}

/// **A `ui_scale` typed at the console is live, from the next frame**: the
/// frame the line ran in keeps the scale its hit tests were made at.
#[test]
fn a_ui_scale_typed_at_the_console_is_the_next_frames() {
    let mut engine = with_console_open();
    assert_eq!(engine.ui_scale(), 1.0);
    run_line(&mut engine, "ui_scale 2");
    assert_eq!(engine.ui_scale(), 1.0, "the frame the line ran in moved");
    step(&mut engine);
    assert_eq!(engine.ui_scale(), 2.0, "the console's write was not live");
    assert_eq!(engine.gpu.draw_list.scale(), 2.0);
}
