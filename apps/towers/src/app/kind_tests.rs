//! **Every tower kind has every row a kind needs** — the one test a new
//! [`tower::Kind`] has to pass, walking [`tower::ALL`] through each table keyed
//! by kind. `crate::tower`'s module docs, _Adding a tower kind_, list the same
//! places in the order to fill them.
//!
//! Most of those places the compiler already holds: [`tower::TOWERS`],
//! [`ACTION_KINDS`], [`KIND_KEYS`], [`tower::ALL`] and [`Sound::ALL`] are
//! arrays as long as [`tower::KINDS`] says, and [`tower::Kind::label`], the
//! audio's waveforms and `crate::map`'s tower tints are `match`es with no
//! wildcard, so a kind missing from any of them does not build. What is left
//! is data — the icon sheet, the replicated numbers' schema, the save and the
//! command frame's bytes — and what this test asks of each is that the kind
//! goes through it and comes back as itself.

use crcbl::input::Binding;
use crcbl::math::DVec3;
use crcbl::ui::image::ImageAtlas;

use super::{ACTION_KINDS, KIND_KEYS, action_map, built_on_the_hud_line};
use crate::art::Icons;
use crate::audio::Audio;
use crate::cue::{Cue, Sound};
use crate::map::{self, Map};
use crate::save::{PLAYER_FILE, Vault};
use crate::tower::{self, Tier};
use crate::{Controls, DEFAULT_TICK_HZ, Game};

/// The plot each kind is built on: the committed field's first.
const PLOT: u8 = 0;

/// Ticks enough for a build to be sent, admitted, built and replicated back.
const SETTLE_TICKS: usize = 3;

/// **Each kind is one of its own in every table, and crosses every wire as
/// itself** — see the module docs for which tables, and why these.
#[test]
fn every_tower_kind_has_every_row() {
    let map = Map::built_in();
    let mut images = ImageAtlas::new();
    let icons = Icons::register(&mut images).expect("an empty atlas has room for the icons");
    let scene = map.scene();
    let palette = scene.materials;
    assert!(
        palette.len() <= scene.capacities.materials as usize,
        "the palette's {} rows do not fit the {} the map reserves",
        palette.len(),
        scene.capacities.materials,
    );
    let actions = action_map();
    let mut audio = Audio::offline();
    let dir = tempfile::tempdir().expect("a scratch directory");
    let vault = Vault::at(dir.path().to_path_buf(), PLAYER_FILE);

    let mut labels = Vec::new();
    let mut keys = Vec::new();
    let mut seen_icons = Vec::new();
    let mut materials = Vec::new();
    for (at, kind) in tower::ALL.into_iter().enumerate() {
        let name = kind.label();

        // Its place in the table, and the one byte a command or a save names it by.
        assert_eq!(kind.index(), at, "{name} is not at its own index in ALL");
        let byte = u8::try_from(at).expect("the kind table fits a byte");
        assert_eq!(tower::Kind::from_index(byte), Some(kind), "{name}'s byte");

        // Its label: one lowercase word, because it is an icon frame's name, an
        // action's suffix and a `[HUD]` key the browser gate matches as `\bname:`.
        assert!(
            !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_lowercase()),
            "{name:?} is not one lowercase word"
        );
        assert!(!labels.contains(&name), "two kinds are called {name}");
        labels.push(name);

        // Its price, at every tier, and something it does there.
        for tier in [Tier::Base, Tier::Upgraded] {
            let spec = kind.spec(tier);
            assert!(spec.cost > 0, "{name} {} costs nothing", tier.label());
            assert!(
                spec.fires() != spec.slows(),
                "{name} {} fires and holds, or does neither",
                tier.label()
            );
        }

        // Its key, and the action it is bound under.
        assert!(
            ACTION_KINDS[at].ends_with(name),
            "action {} is not {name}'s",
            ACTION_KINDS[at]
        );
        assert_eq!(
            actions.bindings(ACTION_KINDS[at]),
            Some([Binding::Key(KIND_KEYS[at])].as_slice()),
            "{name}'s action is not bound to its key"
        );
        assert!(!keys.contains(&KIND_KEYS[at]), "{name} shares a key");
        keys.push(KIND_KEYS[at]);

        // Its icon: a frame of the sheet named after it, one of its own.
        let icon = icons.kind(kind);
        assert!(
            !seen_icons.contains(&icon),
            "{name} has another kind's icon"
        );
        seen_icons.push(icon);

        // Its material: a row of the palette, one of its own.
        let row = map::tower_material(kind);
        assert!(
            row < palette.len(),
            "{name}'s material {row} is past the palette"
        );
        assert!(!materials.contains(&row), "{name} shares material {row}");
        materials.push(row);

        // Its shot's sound: in `Sound::ALL` at its own index, banked and played.
        let fire = Sound::Fire(kind);
        assert_eq!(
            Sound::ALL[fire.index()],
            fire,
            "{name}'s sound is out of place"
        );
        audio.play(Cue {
            sound: fire,
            at: DVec3::ZERO,
        });
        assert_eq!(audio.plays(fire), 1, "{name}'s shot is not banked");

        // Built through the command frame, for exactly its price: a purse of
        // the base cost and nothing more, so the build leaves it empty.
        let mut game = Game::new(DEFAULT_TICK_HZ, &map).expect("a solo game always starts");
        let mut purse = game.checkpoint().expect("the first build phase saves");
        purse.gold = kind.spec(Tier::Base).cost;
        game.restore(&purse)
            .expect("the game's own checkpoint restores");
        game.set_controls(Controls {
            place: Some(PLOT),
            kind,
            ..Controls::default()
        });
        for _ in 0..SETTLE_TICKS {
            game.tick();
        }
        let stats = game.stats();
        assert_eq!(
            stats.built_of(kind),
            1,
            "a {name} build did not cross the command frame"
        );
        assert_eq!(stats.gold, 0, "a {name} build was not charged its price");

        // On the `[HUD]` line, by name.
        assert!(
            built_on_the_hud_line(&stats.built_by_kind).contains(&format!("{name}: 1")),
            "the [HUD] line does not count {name}"
        );

        // Replicated: the tower and the count, read back by solo's own client
        // through the same schema a joiner's reads.
        let seen = game.replicated();
        assert_eq!(seen.undecodable, 0, "a {name} field did not decode");
        assert_eq!(
            seen.render.towers[usize::from(PLOT)].map(|tower| tower.kind),
            Some(kind),
            "a {name} tower did not cross the snapshot"
        );
        assert_eq!(
            seen.stats.built_of(kind),
            1,
            "the snapshot's numbers do not count {name}"
        );

        // Saved and read back as itself.
        let saved = game.checkpoint().expect("the build phase saves");
        vault
            .store(&saved)
            .expect("the scratch directory is writable");
        assert_eq!(
            vault
                .load(&map)
                .expect("a save this build wrote reads back"),
            Some(saved),
            "a {name} tower did not survive the save"
        );
    }
}

/// **The counts read as they did when the `[HUD]` line and the summary named
/// each kind by hand**, now that both walk [`tower::ALL`]: the browser gate
/// matches `\bsplash: (\d+)` on the line, and a reworded one would leave the
/// gate reading nothing. A prefix, so a kind added after these three keeps it.
#[test]
fn the_counts_are_spelled_as_the_line_always_spelled_them() {
    let built: [u64; tower::KINDS] = std::array::from_fn(|at| at as u64);
    let line = built_on_the_hud_line(&built);
    assert!(line.starts_with("bolt: 0  splash: 1  slow: 2"), "{line}");
    let summary = super::built_in_the_summary(&built);
    assert!(summary.starts_with("0 bolt, 1 splash, 2 slow"), "{summary}");
}
