//! The sandbox's scene opened from a directory and reloaded from it: the
//! committed scene is the built-in one, and editing one chunk reloads that
//! system alone.

use std::collections::BTreeMap;

use crcbl::assets::watch::POLL_INTERVAL;
use crcbl::render::{Camera, DirectionalLight};
use crcbl::scene::scn::{Env, EnvCamera};

use super::*;
use crate::scene::Scene;

/// `assets/scenes/cube.scn`, relative to the sandbox's manifest.
const COMMITTED: &str = "assets/scenes/cube.scn";

/// The files a scene directory holds, by key.
const KEYS: [&str; 4] = ["scene.ron", "env.ron", "sys/spin.ron", "sys/sun.ron"];

/// One tick of the sandbox's default rate.
const TICK: f64 = 1.0 / 60.0;

fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(COMMITTED)
}

/// The committed scene's files, as text.
fn committed() -> BTreeMap<String, String> {
    KEYS.iter()
        .map(|key| {
            let text = std::fs::read_to_string(committed_dir().join(key))
                .unwrap_or_else(|error| panic!("{COMMITTED}/{key}: {error}"));
            ((*key).to_owned(), text)
        })
        .collect()
}

/// The scene [`Scene::new`] builds, as the writer writes it: the cube and the
/// sun under ids 0 and 1, the camera the sandbox opens on, and the sun's
/// ambient.
fn built_in_written_out() -> BTreeMap<String, String> {
    let light = DirectionalLight::default();
    let mut scene = Scene::new(light, None);
    let mut ids = IdMap::new();
    ids.assign(scene.cube());
    ids.assign(scene.sun());
    let camera = Camera::default();
    let env = Env {
        camera: EnvCamera {
            position: camera.eye.to_array(),
            look_at: camera.target.to_array(),
        },
        ambient: light.ambient.to_array(),
    };
    scn::Scene::new("cube", vec![SPIN.to_owned(), SUN.to_owned()], env)
        .save(scene.world_mut(), &ids, &codecs())
        .expect("the built-in scene writes")
}

/// The committed scene copied into a fresh directory, to edit.
fn copied() -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::tempdir().expect("a temporary directory");
    let dir = base.path().join("cube.scn");
    std::fs::create_dir_all(dir.join("sys")).expect("the scene's directories");
    for (key, text) in committed() {
        std::fs::write(dir.join(key), text).expect("a scene file");
    }
    (base, dir)
}

/// Polls `scene`'s watch an interval at a time until something reloads, or
/// panics.
fn until_reloaded(scene: &mut Scene) -> Vec<Reload> {
    for _ in 0..8 {
        let reloads = scene.poll_disk(POLL_INTERVAL);
        if !reloads.is_empty() {
            return reloads;
        }
    }
    panic!("two seconds of polling reloaded nothing");
}

/// **The committed scene is the built-in one, written by the writer**, so
/// `--scene` on it starts where a plain run does — and it is maintained by
/// the writer, for the reason `apps/breakout`'s committed board is.
#[test]
fn the_committed_scene_is_the_built_in_one_written_out() {
    assert_eq!(committed(), built_in_written_out());
}

/// **The committed scene opens as the built-in one**: the same light, a cube
/// that has not spun yet.
#[test]
fn the_committed_scene_opens_as_the_built_in_one() {
    let mut scene = Scene::open(&committed_dir(), None).expect("the committed scene opens");
    assert_eq!(scene.light(), Some(DirectionalLight::default()));
    assert_eq!(scene.cube_seconds(), Some(0.0));
    assert!(scene.poll_disk(POLL_INTERVAL).is_empty());
}

/// **Editing one chunk reloads only its system**: the sun takes the file's
/// colour, under the same entity, and the cube — its entity, and the seconds
/// it has spun through since the scene opened — is exactly where it was.
#[test]
fn editing_one_chunk_reloads_only_that_system() {
    let (_base, dir) = copied();
    let mut scene = Scene::open(&dir, None).expect("the copy opens");
    for _ in 0..30 {
        scene.tick(TICK);
    }
    let (cube, sun) = (scene.cube(), scene.sun());
    let spun = scene.cube_seconds().expect("a cube");
    assert!(spun > 0.0, "the cube did not spin");

    let light = DirectionalLight::default();
    let sun_text = std::fs::read_to_string(dir.join("sys/sun.ron")).expect("the sun's chunk");
    let edited = sun_text.replace(&format!("color: ({:?},", light.color.x), "color: (0.25,");
    assert_ne!(edited, sun_text, "the edit changed nothing");
    std::fs::write(dir.join("sys/sun.ron"), edited).expect("the edit");

    let reloads = until_reloaded(&mut scene);
    assert_eq!(reloads.len(), 1, "{reloads:?}");
    assert_eq!(reloads[0].system, SUN);
    let diff = reloads[0].outcome.as_ref().expect("the edit reads");
    assert_eq!(diff.changes().len(), 1);

    let relit = scene.light().expect("a sun");
    assert_eq!(
        relit.color.x, 0.25,
        "the sun did not take the file's colour"
    );
    assert_eq!(relit.direction, light.direction);
    assert_eq!(scene.sun(), sun, "the sun was re-made, not changed");
    assert_eq!(scene.cube(), cube, "the cube was re-made");
    assert_eq!(
        scene.cube_seconds(),
        Some(spun),
        "the cube's spin was reset"
    );
    assert!(scene.poll_disk(POLL_INTERVAL).is_empty());
}

/// **A chunk that will not read keeps the last good state**, and the
/// reload says why, naming the file.
#[test]
fn a_chunk_that_will_not_read_keeps_the_last_good_state() {
    let (_base, dir) = copied();
    let mut scene = Scene::open(&dir, None).expect("the copy opens");
    std::fs::write(
        dir.join("sys/sun.ron"),
        "Chunk(system: \"sun\", entities: [(1, Sun(",
    )
    .expect("half a file");

    let reloads = until_reloaded(&mut scene);
    match &reloads[0].outcome {
        Err(ScnError::Parse { key, .. }) => assert_eq!(key, "sys/sun.ron"),
        other => panic!("half a file was not refused: {other:?}"),
    }
    assert_eq!(
        scene.light(),
        Some(DirectionalLight::default()),
        "the light moved"
    );
}

/// **A directory that is no scene is refused at the start**, naming the
/// file it could not read.
#[test]
fn a_directory_that_is_no_scene_is_refused() {
    let base = tempfile::tempdir().expect("a temporary directory");
    let error = Scene::open(base.path(), None).expect_err("an empty directory");
    assert!(error.to_string().contains("scene.ron"), "{error}");
}
