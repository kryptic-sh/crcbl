//! A chunk reload over a scene of two systems, one entity in both: what moves,
//! what stays put to the handle, and what a bad file leaves.

use std::hash::Hasher;

use crcbl_assets::MemorySource;
use crcbl_ecs::{ComponentHash, Entity, System};
use serde::{Deserialize, Serialize};

use super::*;
use crate::scn::{EntityName, chunk_of};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Mark {
    position: [f32; 3],
    label: String,
}

impl ComponentHash for Mark {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        for value in self.position {
            hasher.write(&value.to_bits().to_le_bytes());
        }
        hasher.write(self.label.as_bytes());
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Glow {
    power: f32,
}

impl ComponentHash for Glow {
    fn hash_component(&self, hasher: &mut dyn Hasher) {
        hasher.write(&self.power.to_bits().to_le_bytes());
    }
}

const DIR: &str = "two.scn";
const MARKS_KEY: &str = "two.scn/sys/marks.ron";

const HEADER: &str =
    "Scene(format: 0, name: \"two\", systems: [\"marks\", \"glows\"], names: true)";
const ENV: &str = "Env(camera: Camera(position: (0.0, 2.0, 8.0), look_at: (0.0, 0.0, 0.0)), \
                   ambient: (0.05, 0.05, 0.08))";
const NAMES: &str = "Names(names: [(2, \"Lamp\")])";

/// Marks 0, 1 and 2; entity 1 is in both systems.
const MARKS: &str = "Chunk(system: \"marks\", entities: [\
    (0, Mark(position: (0.0, 0.0, 0.0), label: \"zero\")),\
    (1, Mark(position: (1.0, 0.0, 0.0), label: \"one\")),\
    (2, Mark(position: (2.0, 0.0, 0.0), label: \"two\")),\
])";

/// Glows 1 and 3.
const GLOWS: &str = "Chunk(system: \"glows\", entities: [\
    (1, Glow(power: 1.0)),\
    (3, Glow(power: 3.0)),\
])";

/// A loaded scene of [`MARKS`] and [`GLOWS`], and the source it came from.
struct Loaded {
    source: MemorySource,
    scene: Scene,
    ids: IdMap,
    world: World,
}

impl Loaded {
    fn new() -> Self {
        let mut source = MemorySource::new();
        for (key, text) in [
            ("two.scn/scene.ron", HEADER),
            ("two.scn/env.ron", ENV),
            ("two.scn/names.ron", NAMES),
            (MARKS_KEY, MARKS),
            ("two.scn/sys/glows.ron", GLOWS),
        ] {
            write(&mut source, key, text);
        }
        let mut world = World::new();
        world.register_system(Box::new(System::<Mark>::new("marks")));
        world.register_system(Box::new(System::<Glow>::new("glows")));
        let (scene, ids) =
            Scene::load(&source, Path::new(DIR), &codecs(), &mut world).expect("the scene loads");
        Self {
            source,
            scene,
            ids,
            world,
        }
    }

    /// Writes `text` as the marks chunk and reloads it.
    fn reload_marks(&mut self, text: &str) -> Result<ChunkDiff, ScnError> {
        write(&mut self.source, MARKS_KEY, text);
        self.scene.reload_chunk(
            &self.source,
            Path::new(DIR),
            "marks",
            &codecs(),
            &mut self.world,
            &mut self.ids,
        )
    }

    fn entity(&self, id: u32) -> Option<Entity> {
        self.ids.entity(SceneEntityId(id))
    }

    fn mark(&mut self, id: u32) -> Option<Mark> {
        let entity = self.entity(id)?;
        self.world
            .system_mut::<System<Mark>>()
            .and_then(|marks| marks.get(entity).cloned())
    }

    fn glow(&mut self, id: u32) -> Option<Glow> {
        let entity = self.entity(id)?;
        self.world
            .system_mut::<System<Glow>>()
            .and_then(|glows| glows.get(entity).cloned())
    }

    /// The scene as the writer would save it.
    fn files(&mut self) -> BTreeMap<String, String> {
        self.scene
            .save(&mut self.world, &self.ids, &codecs())
            .expect("the scene saves")
    }
}

fn codecs() -> Vec<Box<dyn SystemChunk>> {
    vec![chunk_of::<Mark>("marks"), chunk_of::<Glow>("glows")]
}

fn write(source: &mut MemorySource, key: &str, text: &str) {
    source
        .insert(Path::new(key), text.as_bytes().to_vec())
        .expect("a scene key is a legal asset key");
}

/// **A chunk edit reloads only its own system**: the other system's file
/// saves byte for byte as before, its entities keep their handles, and an
/// unchanged row of the reloaded system is not touched either.
#[test]
fn a_chunk_edit_reloads_only_its_system() {
    let mut loaded = Loaded::new();
    let glows_before = loaded.files()["sys/glows.ron"].clone();
    let handles: Vec<_> = (0..4).map(|id| loaded.entity(id)).collect();
    // A runtime change to the other system, which a reload of this one must
    // leave standing — a reload that re-read the scene would put it back.
    let three = loaded.entity(3).expect("filed");
    loaded
        .world
        .system_mut::<System<Glow>>()
        .and_then(|glows| glows.get_mut(three))
        .expect("a glow")
        .power = 30.0;

    let diff = loaded
        .reload_marks(&MARKS.replace("(1.0, 0.0, 0.0)", "(1.0, 5.0, 0.0)"))
        .expect("the edit reloads");

    assert_eq!(
        diff.changes(),
        [RowChange::Changed {
            id: SceneEntityId(1),
            row: "(position:(1.0,5.0,0.0),label:\"one\")".to_owned(),
        }],
        "one row changed, and only it"
    );
    assert_eq!(diff.system(), "marks");
    assert_eq!(
        loaded.mark(1).expect("still a mark").position,
        [1.0, 5.0, 0.0]
    );
    assert_eq!(
        (0..4).map(|id| loaded.entity(id)).collect::<Vec<_>>(),
        handles,
        "an entity was re-instantiated"
    );
    assert_eq!(
        loaded.glow(3),
        Some(Glow { power: 30.0 }),
        "the other system moved"
    );
    assert_eq!(loaded.glow(1), Some(Glow { power: 1.0 }));
    let glows_after = loaded.files()["sys/glows.ron"].clone();
    assert_eq!(
        glows_after,
        glows_before.replace("3.0", "30.0"),
        "the other system's file is not what it was"
    );
}

/// **A file that says what the system holds changes nothing.**
#[test]
fn an_unchanged_chunk_is_an_empty_difference() {
    let mut loaded = Loaded::new();
    let before = loaded.files();
    // Laid out differently from the file it was loaded from: rows are
    // compared as values, not as text on the page.
    let laid_out = MARKS.replace("),(", "),\n    (");
    assert_ne!(laid_out, MARKS, "the layout did not change");
    let diff = loaded.reload_marks(&laid_out).expect("reloads");
    assert!(diff.is_empty(), "{diff:?}");
    assert_eq!(loaded.files(), before);
}

/// **A removed entity goes and an added one appears**, under the ids the file
/// spells: the removed one's id and name go with it, the added one is filed
/// under its own id, and the kept ones keep their handles.
#[test]
fn a_removed_entity_goes_and_an_added_one_appears() {
    let mut loaded = Loaded::new();
    let (zero, one) = (loaded.entity(0), loaded.entity(1));
    let two = loaded.entity(2).expect("filed");
    let text = MARKS
        .replace("(2, Mark(position: (2.0, 0.0, 0.0), label: \"two\")),", "")
        .replace(
            "])",
            "(7, Mark(position: (7.0, 0.0, 0.0), label: \"seven\")),])",
        );
    let diff = loaded.reload_marks(&text).expect("reloads");

    assert_eq!(
        diff.changes().iter().map(RowChange::id).collect::<Vec<_>>(),
        [SceneEntityId(2), SceneEntityId(7)]
    );
    assert_eq!(loaded.entity(2), None, "the removed id is still filed");
    assert!(!loaded.world.is_alive(two), "the removed entity is alive");
    assert_eq!(
        loaded.scene.entity_name(SceneEntityId(2)),
        None,
        "the removed entity's name stayed, which the save refuses"
    );
    assert_eq!(
        loaded.mark(7).map(|mark| mark.label),
        Some("seven".to_owned()),
        "the added entity is not under its id"
    );
    assert_eq!((loaded.entity(0), loaded.entity(1)), (zero, one));
    // And the scene still saves, and reads back to what was reloaded.
    let files = loaded.files();
    assert!(files["sys/marks.ron"].contains("seven"));
    assert!(!files.contains_key("names.ron"), "the only name went");
}

/// **A row removed from an entity another system holds takes only that
/// row**: the entity stays, under its id, with its other system's row.
#[test]
fn a_row_removed_from_an_entity_in_two_systems_leaves_the_entity() {
    let mut loaded = Loaded::new();
    let one = loaded.entity(1).expect("filed");
    let text = MARKS.replace("(1, Mark(position: (1.0, 0.0, 0.0), label: \"one\")),", "");
    let diff = loaded.reload_marks(&text).expect("reloads");
    assert_eq!(
        diff.changes(),
        [RowChange::Removed {
            id: SceneEntityId(1)
        }]
    );
    assert_eq!(loaded.entity(1), Some(one), "the entity went with its row");
    assert!(loaded.world.is_alive(one));
    assert_eq!(loaded.mark(1), None, "the row stayed");
    assert_eq!(
        loaded.glow(1),
        Some(Glow { power: 1.0 }),
        "its other row went"
    );
}

/// **A row added under an id another system holds joins that entity**
/// rather than spawning a second one under the same id.
#[test]
fn a_row_added_under_a_held_id_joins_its_entity() {
    let mut loaded = Loaded::new();
    let three = loaded.entity(3).expect("filed");
    let count = loaded.world.entity_count();
    let text = MARKS.replace(
        "])",
        "(3, Mark(position: (3.0, 0.0, 0.0), label: \"three\")),])",
    );
    loaded.reload_marks(&text).expect("reloads");
    assert_eq!(loaded.entity(3), Some(three));
    assert_eq!(
        loaded.world.entity_count(),
        count,
        "a second entity was spawned"
    );
    assert_eq!(
        loaded.mark(3).map(|mark| mark.label),
        Some("three".to_owned())
    );
}

/// **A chunk that will not read keeps the last good state** and says why,
/// naming the file — a write caught half-finished being the case.
#[test]
fn a_chunk_that_will_not_read_keeps_the_last_good_state() {
    let mut loaded = Loaded::new();
    let before = loaded.files();
    let half_written = &MARKS[..MARKS.len() / 2];
    let error = loaded.reload_marks(half_written).expect_err("half a file");
    assert!(
        matches!(&error, ScnError::Parse { key, .. } if key == MARKS_KEY),
        "{error}"
    );
    assert_eq!(loaded.files(), before, "a refused reload changed the scene");

    // An id spelled twice is refused with nothing applied, even though the
    // rest of the file is a change.
    let twice = MARKS
        .replace("label: \"zero\"", "label: \"nought\"")
        .replace("(2, Mark", "(1, Mark");
    assert!(matches!(
        loaded.reload_marks(&twice),
        Err(ScnError::DuplicateId { .. })
    ));
    assert_eq!(loaded.files(), before);
}

/// **Only a listed system reloads.**
#[test]
fn a_system_the_manifest_does_not_list_is_refused() {
    let mut loaded = Loaded::new();
    let error = loaded
        .scene
        .reload_chunk(
            &loaded.source,
            Path::new(DIR),
            "lights",
            &codecs(),
            &mut loaded.world,
            &mut loaded.ids,
        )
        .expect_err("no such system");
    assert!(
        matches!(&error, ScnError::Unlisted { system } if system == "lights"),
        "{error}"
    );
}

/// **A name for a kept entity stays.** Only a removal takes a name.
#[test]
fn a_kept_entitys_name_stays() {
    let mut loaded = Loaded::new();
    loaded
        .reload_marks(&MARKS.replace("label: \"two\"", "label: \"deux\""))
        .expect("reloads");
    assert_eq!(
        loaded.scene.entity_name(SceneEntityId(2)),
        Some(&EntityName::new("Lamp").expect("a name"))
    );
}
