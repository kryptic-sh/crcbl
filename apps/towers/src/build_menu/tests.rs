use crcbl::ui::image::ImageAtlas;

use super::*;
use crate::map::Map;
use crate::tower::{Kind, TowerView};

/// The default window's size.
const EXTENT: (u32, u32) = (960, 720);

/// The committed field's plots.
fn plots() -> Vec<Plot> {
    Map::built_in().plots().to_vec()
}

/// The icons, registered into an atlas of their own.
fn icons() -> Icons {
    Icons::register(&mut ImageAtlas::new()).expect("an empty page has room")
}

/// A field with `gold` in the purse and `towers` standing, plot by plot.
fn field(gold: u32, towers: &[(usize, Kind, Tier)]) -> RenderState {
    let mut state = RenderState {
        gold,
        ..RenderState::default()
    };
    for &(plot, kind, tier) in towers {
        state.towers[plot] = Some(TowerView {
            kind,
            tier,
            working: false,
        });
    }
    state
}

/// **The ray through a plot's pixel picks that plot, and only that plot** —
/// every plot of the committed field, at its pad and at the top of a tower
/// standing on it, through the overhead camera the frame is drawn with. The
/// window's corner, over nothing but the ground, picks none: the control that
/// says a hit is the box's and not every pixel's.
#[test]
fn the_ray_through_a_plot_picks_that_plot() {
    let camera = crate::camera::camera();
    let plots = plots();
    for (index, plot) in plots.iter().enumerate() {
        let pad = camera
            .pixel_of(pad_top(plot), EXTENT)
            .expect("the pad is in front of the camera");
        assert_eq!(
            plot_under(&plots, &camera, EXTENT, pad),
            u8::try_from(index).ok(),
            "{}'s pad picks another plot",
            plot.label,
        );
        let post = pad_top(plot) + Vec3::Y * (TOWER_HEIGHT as f32);
        let post = camera
            .pixel_of(post, EXTENT)
            .expect("the post is in front of the camera");
        assert_eq!(
            plot_under(&plots, &camera, EXTENT, post),
            u8::try_from(index).ok(),
            "{}'s tower does not pick its plot",
            plot.label,
        );
    }
    assert_eq!(
        plot_under(&plots, &camera, EXTENT, Vec2::new(2.0, 2.0)),
        None
    );
}

/// **A free plot offers every kind at its own price, and the purse greys what
/// it cannot reach** — with exactly a slow tower's price in it, so the slow
/// tower is the boundary: affordable at the price, where a splash tower is
/// not.
#[test]
fn a_free_plot_offers_every_kind_and_greys_what_the_purse_cannot_reach() {
    let icons = icons();
    let gold = Kind::Slow.spec(Tier::Base).cost;
    assert!(
        Kind::Splash.spec(Tier::Base).cost > gold && Kind::Bolt.spec(Tier::Base).cost < gold,
        "the prices no longer straddle the slow tower's, so this test proves less",
    );
    let offered = entries(&field(gold, &[]), 1, &icons);
    assert_eq!(offered.len(), tower::KINDS);
    for (entry, kind) in offered.iter().zip(tower::ALL) {
        assert_eq!(entry.label, kind.label().to_uppercase());
        assert_eq!(entry.price, Some(kind.spec(Tier::Base).cost));
        assert_eq!(entry.pick, Pick::Build { plot: 1, kind });
        assert_eq!(entry.icon, icons.kind(kind));
        assert_eq!(
            entry.enabled,
            kind != Kind::Splash,
            "{} is offered wrongly against {gold} gold",
            kind.label(),
        );
    }
}

/// **A base tower offers its own upgrade at its own price; an upgraded one
/// offers nothing to buy.**
#[test]
fn a_tower_offers_its_upgrade_until_there_is_none() {
    let icons = icons();
    let price = Kind::Splash.spec(Tier::Upgraded).cost;
    let base = entries(&field(price, &[(3, Kind::Splash, Tier::Base)]), 3, &icons);
    assert_eq!(
        base,
        [Entry {
            label: UPGRADE.to_string(),
            price: Some(price),
            enabled: true,
            pick: Pick::Upgrade { plot: 3 },
            icon: icons.upgrade(),
        }],
    );
    let short = entries(
        &field(price - 1, &[(3, Kind::Splash, Tier::Base)]),
        3,
        &icons,
    );
    assert!(
        !short[0].enabled,
        "an upgrade the purse cannot reach is offered"
    );

    let maxed = entries(&field(999, &[(3, Kind::Splash, Tier::Upgraded)]), 3, &icons);
    assert_eq!(maxed.len(), 1);
    assert_eq!(maxed[0].label, MAX_TIER);
    assert_eq!(maxed[0].price, None);
    assert!(!maxed[0].enabled, "a maxed tower offers something to buy");
}

/// **A tap that came in one batch is a press on one frame and a release on
/// the next.** The tree reads a press off the button being held, so a press
/// and release handed over together would be a tap that never happened.
#[test]
fn a_tap_in_one_batch_is_a_press_then_a_release() {
    let mut pointer = Pointer::default();
    pointer.fold(PointerUpdate {
        at: Some(Vec2::ZERO),
        pressed: true,
        released: true,
        ..PointerUpdate::default()
    });
    let (first, pressed) = pointer.take(EXTENT);
    assert!(pressed, "the press was lost");
    assert!(
        first.down && !first.released,
        "the press frame is {first:?}"
    );
    assert_eq!(
        first.pos,
        Vec2::new(480.0, 360.0),
        "the centre of the window"
    );
    let (second, pressed) = pointer.take(EXTENT);
    assert!(!pressed);
    assert!(
        !second.down && second.released,
        "the release frame is {second:?}"
    );
    let (idle, _) = pointer.take(EXTENT);
    assert!(!idle.down && !idle.released, "the tap left the button held");
}
