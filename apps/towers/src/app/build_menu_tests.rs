//! The build menu through the running game, headless: a click or a tap on a
//! plot opens it, a pick in it is the command the keys send, a click elsewhere
//! closes it — and the keyboard and the stage are untouched by it.

use crcbl::core::input::KeyCode;
use crcbl::engine::ExitReason;
use crcbl::math::Vec2;
use crcbl::shell::{ButtonState, HeadlessShell, PhysicalPoint, PointerButton};
use crcbl::ui::draw_list::DrawCommand;

use super::tests::{frames, headless, scripted, tap};
use super::{KIND_KEYS, Loop};
use crate::build_menu::{HOVER, MAX_TIER, Pick, UPGRADE, pad_top, plot_under};
use crate::tower::{self, Kind, Tier};

/// Where `plot`'s pad is on the screen, through the camera the frame is drawn
/// from.
fn plot_pixel(engine: &Loop<HeadlessShell>, plot: u8) -> Vec2 {
    let pad = pad_top(&engine.game().game().map().plots()[usize::from(plot)]);
    engine
        .game()
        .dev_camera()
        .camera()
        .pixel_of(pad, engine.gpu().extent())
        .expect("the pad is in front of the camera")
}

/// The middle of item `index` of the open menu, as it was last laid out.
fn item_pixel(engine: &Loop<HeadlessShell>, index: usize) -> Vec2 {
    let (min, max) = engine
        .game()
        .build_menu()
        .item_rect(index)
        .expect("the menu is open with that item on it");
    (min + max) * 0.5
}

/// A point over the field that is over no plot and well clear of an open
/// menu: the bottom right of the window.
fn nowhere(engine: &Loop<HeadlessShell>) -> Vec2 {
    let extent = engine.gpu().extent();
    let at = Vec2::new(extent.0 as f32 - 8.0, extent.1 as f32 * 0.5);
    let camera = engine.game().dev_camera().camera();
    assert_eq!(
        plot_under(engine.game().game().map().plots(), &camera, extent, at),
        None,
        "the point meant to be over no plot is over one",
    );
    at
}

fn physical(at: Vec2) -> PhysicalPoint {
    PhysicalPoint {
        x: f64::from(at.x),
        y: f64::from(at.y),
    }
}

/// Moves the pointer to `at` and runs a frame.
fn hover(engine: &mut Loop<HeadlessShell>, at: Vec2) {
    let window = engine.window();
    engine
        .shell_mut()
        .move_pointer(window, physical(at), (0.0, 0.0))
        .expect("the window is live");
    frames(engine, 1);
}

/// Clicks at `at`: the press on one frame, the release on the next, as a
/// mouse does.
fn click(engine: &mut Loop<HeadlessShell>, at: Vec2) {
    let window = engine.window();
    engine
        .shell_mut()
        .move_pointer(window, physical(at), (0.0, 0.0))
        .expect("the window is live");
    for state in [ButtonState::Pressed, ButtonState::Released] {
        engine
            .shell_mut()
            .button(window, PointerButton::Left, state, Some(physical(at)))
            .expect("the window is live");
        frames(engine, 1);
    }
}

/// Taps at `at` the way a phone does: the press and the release in one
/// frame's batch.
fn tap_at(engine: &mut Loop<HeadlessShell>, at: Vec2) {
    let window = engine.window();
    engine
        .shell_mut()
        .move_pointer(window, physical(at), (0.0, 0.0))
        .expect("the window is live");
    for state in [ButtonState::Pressed, ButtonState::Released] {
        engine
            .shell_mut()
            .button(window, PointerButton::Left, state, Some(physical(at)))
            .expect("the window is live");
    }
    frames(engine, 1);
}

/// The index of the item that builds `kind`, on a free plot's menu.
fn item_of(engine: &Loop<HeadlessShell>, kind: Kind) -> usize {
    engine
        .game()
        .build_menu()
        .offered()
        .iter()
        .position(
            |entry| matches!(entry.pick, Pick::Build { kind: offered, .. } if offered == kind),
        )
        .expect("the free plot's menu offers every kind")
}

/// A run four frames in, nothing pressed.
fn playing() -> Loop<HeadlessShell> {
    let mut engine = scripted(&headless(2000));
    frames(&mut engine, 4);
    engine
}

/// **A click on a free plot opens the menu there, offering every kind at its
/// price with its icon** — and once a splash tower has spent the purse below
/// a splash tower's price, the next plot's menu offers it greyed, and a click
/// on it builds nothing.
#[test]
fn a_click_on_a_free_plot_opens_the_menu_with_every_kind_priced() {
    let mut engine = playing();
    assert_eq!(engine.game().build_menu().open_on(), None);
    let at = plot_pixel(&engine, 2);
    click(&mut engine, at);
    let menu = engine.game().build_menu();
    assert_eq!(
        menu.open_on(),
        Some(2),
        "the click opened no menu on its plot"
    );
    let offered = menu.offered();
    assert_eq!(
        offered
            .iter()
            .map(|entry| entry.label.clone())
            .collect::<Vec<_>>(),
        tower::ALL.map(|kind| kind.label().to_uppercase()),
    );
    for (entry, kind) in offered.iter().zip(tower::ALL) {
        assert_eq!(entry.price, Some(kind.spec(Tier::Base).cost));
        assert!(entry.enabled, "{} is greyed in a full purse", kind.label());
    }
    // Every kind's icon reached the frame's draw list, at its own place on
    // the atlas page.
    let drawn: Vec<Vec2> = engine
        .gpu()
        .draw_list()
        .commands()
        .iter()
        .filter_map(|command| match command {
            DrawCommand::Image { uv_min, .. } => Some(*uv_min),
            _ => None,
        })
        .collect();
    for kind in tower::ALL {
        let icon = engine.game().build_menu().icons().kind(kind);
        assert!(
            drawn.contains(&icon.uv_min()),
            "the {} icon is not on the menu",
            kind.label(),
        );
    }

    let at = item_pixel(&engine, item_of(&engine, Kind::Splash));

    click(&mut engine, at);
    frames(&mut engine, 1);
    let spent = engine.game().game().stats();
    assert_eq!(
        spent.built_of(Kind::Splash),
        1,
        "the pick built no splash tower"
    );
    assert!(
        spent.gold < Kind::Splash.spec(Tier::Base).cost,
        "the purse still reaches a second splash tower, so the rest proves nothing",
    );

    let at = plot_pixel(&engine, 3);

    click(&mut engine, at);
    let splash = item_of(&engine, Kind::Splash);
    assert!(
        !engine.game().build_menu().offered()[splash].enabled,
        "a splash tower the purse cannot reach is offered",
    );
    assert!(engine.game().build_menu().offered()[item_of(&engine, Kind::Bolt)].enabled);
    let at = item_pixel(&engine, splash);
    click(&mut engine, at);
    frames(&mut engine, 2);
    let after = engine.game().game().stats();
    assert_eq!(after.towers, spent.towers, "a greyed item built a tower");
    assert_eq!(after.refused, spent.refused, "a greyed item sent a command");
    assert_eq!(engine.game().build_menu().open_on(), Some(3));
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// How many ticks had run when `engine`'s first tower went up, stepping a
/// frame at a time from where it is.
fn tick_of_the_first_build(engine: &mut Loop<HeadlessShell>) -> u64 {
    for _ in 0..8 {
        if engine.game().game().stats().towers > 0 {
            return engine.game().game().ticks_run();
        }
        frames(engine, 1);
    }
    panic!("no tower went up");
}

/// **A pick sends exactly the command the keys send.** One run picks a splash
/// tower on the third plot from the menu; another walks the cursor there with
/// the arrows, picks the kind with its digit and presses `B` on the tick the
/// pick was sealed on. Both stages then agree, tick for tick — the same
/// tower of the same kind on the same plot, the same purse, the same field
/// for a hundred ticks after — which nothing but the same four bytes sent on
/// the same tick would make them do.
#[test]
fn a_pick_sends_exactly_the_command_the_keys_send() {
    const PLOT: u8 = 2;
    const KIND: Kind = Kind::Splash;
    /// The frames the keyed run's arrows and digit take: two a tap.
    const KEYED_LEAD: usize = 2 * (PLOT as usize + 1);

    let mut clicked = playing();
    // Long enough for the keyed run's cursor and kind to be in place by the
    // tick this run's pick is sealed on.
    frames(&mut clicked, KEYED_LEAD);
    let at = plot_pixel(&clicked, PLOT);
    click(&mut clicked, at);
    let at = item_pixel(&clicked, item_of(&clicked, KIND));
    click(&mut clicked, at);
    let built_at = tick_of_the_first_build(&mut clicked);
    let tower = clicked.game().game().render_state().towers[usize::from(PLOT)]
        .expect("the pick built on its plot");
    assert_eq!((tower.kind, tower.tier), (KIND, Tier::Base));

    let mut keyed = playing();
    for _ in 0..PLOT {
        tap(&mut keyed, KeyCode::ArrowRight);
    }
    tap(&mut keyed, KIND_KEYS[KIND.index()]);
    assert!(
        keyed.game().game().ticks_run() < built_at,
        "the keys cannot reach the pick's tick: the test needs a later pick",
    );
    while keyed.game().game().ticks_run() + 1 < built_at {
        frames(&mut keyed, 1);
    }
    tap(&mut keyed, KeyCode::KeyB);
    frames(&mut clicked, 1);
    assert_eq!(
        keyed.game().game().ticks_run(),
        clicked.game().game().ticks_run()
    );
    for _ in 0..100 {
        assert_eq!(
            keyed.game().game().stage_fingerprint(),
            clicked.game().game().stage_fingerprint(),
            "the stages parted at tick {}",
            keyed.game().game().ticks_run(),
        );
        frames(&mut keyed, 1);
        frames(&mut clicked, 1);
    }
    assert_eq!(keyed.game().game().stats(), clicked.game().game().stats());
}

/// **A click on a built tower offers its upgrade, at its price, and the pick
/// upgrades it** — the purse paying the upgrade's own price, and the menu on
/// the stepped-up tower offering nothing more to buy.
#[test]
fn a_click_on_a_built_tower_offers_its_upgrade_and_the_pick_upgrades_it() {
    let mut engine = playing();
    tap(&mut engine, KeyCode::KeyB);
    frames(&mut engine, 1);
    assert_eq!(
        engine.game().game().stats().towers,
        1,
        "the key built nothing"
    );

    let at = plot_pixel(&engine, 0);

    click(&mut engine, at);
    let price = Kind::Bolt.spec(Tier::Upgraded).cost;
    let offered = engine.game().build_menu().offered().to_vec();
    assert_eq!(offered.len(), 1, "a built tower offers {offered:?}");
    assert_eq!(offered[0].label, UPGRADE);
    assert_eq!(offered[0].price, Some(price));
    assert_eq!(offered[0].pick, Pick::Upgrade { plot: 0 });

    let purse = engine.game().game().stats().gold;
    let at = item_pixel(&engine, 0);
    click(&mut engine, at);
    frames(&mut engine, 1);
    let stats = engine.game().game().stats();
    assert_eq!(stats.upgrades, 1, "the pick upgraded nothing");
    assert_eq!(stats.refused, 0);
    assert_eq!(
        stats.gold,
        purse - price,
        "the purse did not pay the upgrade"
    );

    let at = plot_pixel(&engine, 0);

    click(&mut engine, at);
    let maxed = engine.game().build_menu().offered().to_vec();
    assert_eq!(maxed.len(), 1);
    assert_eq!(maxed[0].label, MAX_TIER);
    assert!(!maxed[0].enabled);
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A click elsewhere closes the menu and sends nothing** — on open ground,
/// and on another plot, where the click is spent closing the menu rather than
/// opening another. A click on a plot with the menu closed opens it again: the
/// control that says closing is not for good.
#[test]
fn a_click_elsewhere_closes_the_menu_and_sends_nothing() {
    let mut engine = playing();
    let before = engine.game().game().stats();
    let at = plot_pixel(&engine, 1);
    click(&mut engine, at);
    assert_eq!(engine.game().build_menu().open_on(), Some(1));

    let at = nowhere(&engine);

    click(&mut engine, at);
    assert_eq!(
        engine.game().build_menu().open_on(),
        None,
        "open ground left it open"
    );

    let at = plot_pixel(&engine, 1);

    click(&mut engine, at);
    assert_eq!(
        engine.game().build_menu().open_on(),
        Some(1),
        "it did not open again"
    );
    let at = plot_pixel(&engine, 4);
    click(&mut engine, at);
    assert_eq!(
        engine.game().build_menu().open_on(),
        None,
        "a click on another plot was not spent closing the menu",
    );
    frames(&mut engine, 2);
    let after = engine.game().game().stats();
    assert_eq!(
        (after.towers, after.upgrades, after.refused, after.gold),
        (before.towers, before.upgrades, before.refused, before.gold),
        "opening and closing the menu sent a command",
    );
}

/// **The keyboard plays as it always did with the menu open**: opening it
/// moves neither the cursor nor the kind, and the keys build where the cursor
/// is — not on the plot the menu is open on.
#[test]
fn keyboard_play_is_unchanged_with_the_menu_open() {
    let mut engine = playing();
    let at = plot_pixel(&engine, 3);
    click(&mut engine, at);
    assert_eq!(engine.game().build_menu().open_on(), Some(3));
    assert_eq!(
        engine.game().selected(),
        0,
        "opening the menu moved the cursor"
    );
    assert_eq!(
        engine.game().kind(),
        Kind::Bolt,
        "opening the menu picked a kind"
    );

    tap(&mut engine, KeyCode::ArrowRight);
    tap(&mut engine, KIND_KEYS[Kind::Slow.index()]);
    tap(&mut engine, KeyCode::KeyB);
    frames(&mut engine, 1);
    let state = engine.game().game().render_state();
    assert_eq!(
        state.towers[1].map(|tower| tower.kind),
        Some(Kind::Slow),
        "the keys did not build at the cursor",
    );
    assert_eq!(state.towers[3], None, "the keys built on the menu's plot");
    engine.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A tap faster than a frame opens the menu**, as a click does — the press
/// and the release in one batch, which is every tap on a phone.
#[test]
fn a_tap_faster_than_a_frame_opens_the_menu() {
    let mut engine = playing();
    let at = plot_pixel(&engine, 2);
    tap_at(&mut engine, at);
    frames(&mut engine, 1);
    assert_eq!(
        engine.game().build_menu().open_on(),
        Some(2),
        "the tap was lost"
    );

    let at = item_pixel(&engine, item_of(&engine, Kind::Bolt));

    tap_at(&mut engine, at);
    frames(&mut engine, 2);
    assert_eq!(
        engine.game().game().stats().built_of(Kind::Bolt),
        1,
        "the tap picked nothing"
    );
}

/// **The pointer over a plot outlines it**, and over open ground outlines
/// nothing.
#[test]
fn the_pointer_over_a_plot_outlines_it() {
    let outlines = |engine: &Loop<HeadlessShell>| {
        engine
            .gpu()
            .draw_list()
            .commands()
            .iter()
            .filter(|command| {
                matches!(command, DrawCommand::Polyline { color, closed: true, .. } if *color == HOVER)
            })
            .count()
    };
    let mut engine = playing();
    assert_eq!(outlines(&engine), 0);
    let at = plot_pixel(&engine, 4);
    hover(&mut engine, at);
    assert_eq!(
        outlines(&engine),
        1,
        "the plot under the pointer is not outlined"
    );
    let at = nowhere(&engine);
    hover(&mut engine, at);
    assert_eq!(outlines(&engine), 0, "open ground is outlined");
}

/// After one frame: how many ticks the game has run, and the stage's
/// fingerprint — `Game::stage_fingerprint`.
type Seen = (u64, Option<(u64, usize)>);

/// The stage, frame by frame, over `count` frames — opening and closing the
/// menu along the way when `menus` says so.
fn fingerprints(count: usize, menus: bool) -> Vec<Seen> {
    let mut engine = playing();
    let mut seen = Vec::with_capacity(count);
    for frame in 0..count {
        if menus && frame % 10 == 0 {
            // Open on a plot, then dismiss on open ground: two frames each.
            let at = if frame % 20 == 0 {
                plot_pixel(&engine, 2)
            } else {
                nowhere(&engine)
            };
            click(&mut engine, at);
        } else if !menus && frame % 10 == 0 {
            frames(&mut engine, 2);
        } else {
            frames(&mut engine, 1);
        }
        seen.push((
            engine.game().game().ticks_run(),
            engine.game().game().stage_fingerprint(),
        ));
    }
    seen
}

/// **Opening and closing the menu leaves the stage exactly as it would have
/// been**, frame for frame, through a wave arriving on its own — and the
/// menu did open, or the two agreeing would say nothing.
#[test]
fn opening_and_closing_the_build_menu_leaves_the_stage_hash_alone() {
    let mut engine = playing();
    let at = plot_pixel(&engine, 2);
    click(&mut engine, at);
    assert_eq!(
        engine.game().build_menu().open_on(),
        Some(2),
        "the menu never opened"
    );

    let quiet = fingerprints(300, false);
    let busy = fingerprints(300, true);
    assert_eq!(quiet.len(), busy.len());
    for (frame, (a, b)) in quiet.iter().zip(&busy).enumerate() {
        assert_eq!(a, b, "the stage diverged on frame {frame}");
    }
}
