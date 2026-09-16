//! The editor's own scene vocabulary, and the document it opens on.
//!
//! **A tool that opens a scene ships with a vocabulary.** A `.scn/` chunk
//! cannot be read without the type its rows are of, so
//! `crcbl::registry::Registry` is what a vocabulary is and this module builds
//! this build's: the editor's own [`Block`], and the components of the two
//! games whose committed scenes the workspace wants edited. It also holds the
//! scene the editor opens when the command line names none.
//!
//! # Why the editor owns a component at all
//!
//! Slice 1's answer was to depend on `apps/breakout` and open its board, which
//! `docs/plan/08-editor.md` and `docs/backlog.md` both record as the only arrow
//! in the workspace pointing at a sample. The registry removes the *need* for
//! that arrow — this crate's [`document`](crate::document) and
//! [`app`](crate::app) name no game — but a tool still has to have something to
//! open. A greybox block is the smallest thing that is honestly the editor's
//! own: it is what `docs/plan/08-editor.md`'s exit criterion "create scene from
//! empty → place meshes" starts from, and it is what [`crate::app`] already
//! draws every entity as.
//!
//! A build of this editor that should open some other game's scene registers
//! that game's components in [`vocabulary`] beside these — one line each, and
//! the same call the game's own loader makes, so the vocabulary a game ships
//! and the vocabulary the editor sees cannot drift. A scene naming a system
//! **this** build did not register is refused by name rather than opened empty;
//! see [`crate::document::Document::open`].

use std::path::Path;

use crcbl::assets::MemorySource;
use crcbl::ecs::ComponentHash;
use crcbl::math::DVec3;
use crcbl::reflect::Reflect;
use crcbl::registry::{Placement, Registry};
use crcbl::serde::{Deserialize, Serialize};

/// The one system this vocabulary is made of: the manifest entry, the chunk
/// file's stem, and the name [`Block`]'s codec and system are registered under.
pub const BLOCKS: &str = "blocks";

/// The directory the compiled-in scene is keyed under in [`built_in_source`].
pub const GREYBOX: &str = "greybox.scn";

/// `greybox.scn/scene.ron`, as [`crcbl::scene::scn::Scene::save`] writes it.
///
/// Compiled in as text rather than built from code, so the default document is
/// reviewable as the thing it is; `the_compiled_in_scene_is_what_the_writer_writes`
/// is what keeps these three strings in the writer's own spelling.
const GREYBOX_SCENE_RON: &str = r#"Scene(
    format: 0,
    name: "greybox",
    systems: [
        "blocks",
    ],
)"#;

/// `greybox.scn/env.ron`, as the writer writes it.
const GREYBOX_ENV_RON: &str = r"Env(
    camera: Camera(
        position: (0.0, 6.0, 14.0),
        look_at: (0.0, 1.0, 0.0),
    ),
    ambient: (0.05, 0.05, 0.06),
)";

/// `greybox.scn/sys/blocks.ron`, as the writer writes it: a ground slab whose top
/// is `y = 0`, and three steps standing on it.
///
/// **The steps are 2.4 m wide on a 3.0 m pitch**, which is deliberate and is the
/// shape `apps/breakout`'s brick grid has: adjacent rows with a gap between them
/// are what let `crate::document`'s tests say a ray picked the entity it points
/// at *and not the one beside it*, and what let a ray down the gap pick nothing.
/// Three heights so the picture says which way is up.
const GREYBOX_BLOCKS_RON: &str = r#"Chunk(
    system: "blocks",
    entities: [
        (0, Block(
            position: (0.0, -0.5, 0.0),
            half_extents: (8.0, 0.5, 8.0),
        )),
        (1, Block(
            position: (-3.0, 0.25, 0.0),
            half_extents: (1.2, 0.25, 1.5),
        )),
        (2, Block(
            position: (0.0, 0.75, 0.0),
            half_extents: (1.2, 0.75, 1.5),
        )),
        (3, Block(
            position: (3.0, 1.25, 0.0),
            half_extents: (1.2, 1.25, 1.5),
        )),
    ],
)"#;

/// A greybox box: where it stands and how far it reaches.
///
/// Both in world units and both `f64`, for `apps/breakout`'s `Brick`'s reason:
/// this is the number the physics world a pick goes through is spelled in —
/// `crcbl::phys::Transform` and `ColliderComponent::Box` take [`DVec3`] — and a
/// scene written as `f32` would round on the way through the file and move the
/// picture.
#[derive(Clone, Copy, Debug, PartialEq, Reflect, Serialize, Deserialize)]
#[reflect(crate = "crcbl::reflect")]
#[serde(crate = "crcbl::serde")]
pub struct Block {
    /// The block's centre.
    #[reflect(name = "Position")]
    pub position: [f64; 3],
    /// Half its extent on each axis, which is the box collider's own shape.
    ///
    /// A half extent is never negative, and the range's far end is where a
    /// greybox block has left any scene a person is looking at. Advisory — it
    /// bounds the widget, not the write.
    #[reflect(name = "Half extents", min = 0.0, max = 64.0, step = 0.01)]
    pub half_extents: [f64; 3],
}

impl ComponentHash for Block {
    fn hash_component(&self, hasher: &mut dyn std::hash::Hasher) {
        for value in self.position.iter().chain(&self.half_extents) {
            hasher.write(&value.to_bits().to_le_bytes());
        }
    }
}

/// A block's own box, which is what its collider already is: the two fields
/// spell a centre and half extents directly.
impl Placement for Block {
    fn placement(&self) -> Option<(DVec3, DVec3)> {
        Some((
            DVec3::from_array(self.position),
            DVec3::from_array(self.half_extents),
        ))
    }
}

/// This build's vocabulary: the components a scene it opens may be made of.
///
/// **The one place `blocks` is joined to [`Block`]**, and the only registration
/// this binary ships with — see the [module docs](self).
#[must_use]
pub fn vocabulary() -> Registry {
    let mut registry = Registry::new();
    registry.register::<Block>(BLOCKS);
    // The games the workspace wants edited, through the call each one's own
    // loader makes: `docs/plan/sample/07-towers.md`'s milestone 2 is an editor
    // dogfood pass, and an editor that could not open a sample's committed
    // scene could not be dogfooded at all.
    crcbl_breakout::register_components(&mut registry);
    crcbl_puppet::map::register_components(&mut registry);
    registry
}

/// The compiled-in scene directory, as a source with no filesystem under it,
/// keyed under [`GREYBOX`].
///
/// Compiled in rather than read off disk for the reason `apps/breakout`'s own
/// loader gives: a tool that had to find its own default document would be one
/// whose behaviour depended on the directory it was started from.
#[must_use]
pub fn built_in_source() -> MemorySource {
    let mut source = MemorySource::new();
    for (key, text) in [
        ("scene.ron", GREYBOX_SCENE_RON),
        ("env.ron", GREYBOX_ENV_RON),
        ("sys/blocks.ron", GREYBOX_BLOCKS_RON),
    ] {
        // `format!` and not `Path::join`: an asset key is `/`-separated on every
        // host, and a key joined on Windows would not be one anywhere else.
        source
            .insert(
                Path::new(&format!("{GREYBOX}/{key}")),
                text.as_bytes().to_vec(),
            )
            .expect("a nested scene key is a legal asset key");
    }
    source
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crcbl::reflect::{Kind, Value, get_path};

    use crate::document::Document;

    /// **The compiled-in scene is what the writer writes**, byte for byte.
    ///
    /// The same discipline `apps/breakout` and `apps/puppet` hold their committed
    /// scenes to, and for the same reason: ron prints a float through Rust's
    /// shortest-round-trip `Display`, so a hand-typed coordinate that is not the
    /// writer's spelling is a file that reads back as a different number than it
    /// looks like. A load and a save of these three strings has to be the
    /// identity.
    #[test]
    fn the_compiled_in_scene_is_what_the_writer_writes() {
        let mut document = Document::open(&built_in_source(), Path::new(GREYBOX), vocabulary())
            .expect("the compiled-in scene is a scene");
        let files = document.files().expect("every block has an id");

        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            ["env.ron", "scene.ron", "sys/blocks.ron"],
            "the manifest's files and nothing else",
        );
        assert_eq!(files["scene.ron"], GREYBOX_SCENE_RON);
        assert_eq!(files["env.ron"], GREYBOX_ENV_RON);
        assert_eq!(files["sys/blocks.ron"], GREYBOX_BLOCKS_RON);
        for (key, text) in &files {
            assert!(!text.contains('\r'), "the newline is pinned in {key}");
        }
    }

    /// **The shipped vocabulary is this build's own component and both games'**,
    /// so the editor opens the scenes the workspace wants edited — the dogfood
    /// pass `docs/plan/sample/07-towers.md`'s milestone 2 asks for — and the
    /// registry resolves each name to the type whose chunk it is.
    #[test]
    fn the_vocabulary_is_this_builds_block_and_both_games_components() {
        let registry = vocabulary();
        assert_eq!(
            registry.systems().collect::<Vec<_>>(),
            [BLOCKS, "bricks", "spawn", "sun", "surfaces"],
            "the shipped build opens its own scene and both samples'",
        );
        for (system, type_name) in [
            (BLOCKS, "Block"),
            ("bricks", "Brick"),
            ("surfaces", "Surface"),
        ] {
            assert!(
                registry
                    .component_type(system)
                    .is_some_and(|name| name.ends_with(type_name)),
                "{system} does not resolve to {type_name}: {:?}",
                registry.component_type(system),
            );
        }
    }

    /// **A game's committed scene opens through the shipped vocabulary.** The
    /// capability claim: `apps/breakout`'s board is what slice 1 opened, and a
    /// registry that did not carry the game's components would refuse it by
    /// name rather than open it.
    #[test]
    fn the_shipped_vocabulary_opens_breakouts_committed_board() {
        let mut document = crate::Document::open(
            &crcbl_breakout::built_in_source(),
            std::path::Path::new(crcbl_breakout::BOARD),
            vocabulary(),
        )
        .expect("the shipped vocabulary knows what a bricks chunk holds");
        let outline = document.outline();
        let bricks = outline
            .iter()
            .find(|(system, _)| system == "bricks")
            .map(|(_, ids)| ids.len())
            .unwrap_or_default();
        assert!(
            bricks > 0,
            "breakout's bricks did not load: {:?}",
            outline
                .iter()
                .map(|(s, ids)| (s, ids.len()))
                .collect::<Vec<_>>(),
        );
    }

    /// A block's rows are the two a panel draws, labelled as the attributes say —
    /// the claim `#[derive(Reflect)]` is carried here for.
    #[test]
    fn a_block_describes_the_two_rows_a_panel_draws() {
        let block = Block {
            position: [1.0, 2.0, 3.0],
            half_extents: [0.5, 0.5, 0.5],
        };
        assert_eq!(block.kind(), Kind::Struct);
        let fields = block.fields();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].label, "Position");
        assert_eq!(fields[1].label, "Half extents");
        assert_eq!(fields[1].step, Some(0.01));
        assert_eq!(get_path(&block, "position.2"), Ok(Value::Float(3.0)));
    }

    /// A block's placement is its own box, which is what a collider and a bounds
    /// box are both built from.
    #[test]
    fn a_blocks_placement_is_its_centre_and_its_half_extents() {
        let block = Block {
            position: [1.0, -2.0, 3.0],
            half_extents: [4.0, 0.5, 6.0],
        };
        assert_eq!(
            block.placement(),
            Some((DVec3::new(1.0, -2.0, 3.0), DVec3::new(4.0, 0.5, 6.0))),
        );
    }
}
