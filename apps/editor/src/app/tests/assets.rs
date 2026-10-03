//! Dragging an asset from the browser into the viewport, and Enter on one, as
//! a window system delivers them: the mesh stands where the drop lands, one
//! undo takes it back, play refuses it, and the new entity is selected.

use super::*;

use crcbl::math::DVec3;
use crcbl::phys::Ray;
use crcbl::scene_mesh::{MESHES, MIN_HALF_EXTENT};

use crate::document::mesh_tests::{TRIANGLE, props};

/// An editor over [`props`], its browser listing the props' assets and its
/// camera framing them.
fn props_editor() -> Editor<HeadlessShell> {
    let mut editor = headless(64);
    editor.document = props();
    editor.panels.relist_assets(&editor.document);
    editor.frame_scene();
    editor.frame().expect("a frame");
    editor
}

/// The middle of the browser row of `asset`, as the last frame laid it out.
pub(super) fn row_of(editor: &Editor<HeadlessShell>, asset: &str) -> Vec2 {
    let key = editor
        .panels
        .asset_rows()
        .into_iter()
        .find_map(|(key, listed)| (listed == asset).then_some(key))
        .unwrap_or_else(|| panic!("the browser drew no row for {asset}"));
    let (min, max) = editor.panels.ui().rect(key).expect("laid out last frame");
    (min + max) * 0.5
}

/// The window pixel the camera draws `point` at.
pub(super) fn pixel_of(editor: &Editor<HeadlessShell>, point: Vec3) -> Vec2 {
    let (corner, _) = editor.panels.viewport_pixels();
    corner
        + editor
            .camera
            .camera()
            .pixel_of(point, editor.panels.viewport_extent())
            .expect("in front of the camera")
}

/// Where the ray through the window pixel `at` crosses the plane `axis =
/// value` — worked out here rather than asked of the document, so a test of
/// where a drop lands does not take the document's word for it.
fn crossing(editor: &Editor<HeadlessShell>, at: Vec2, axis: usize, value: f64) -> DVec3 {
    let ray = editor.ray_at(at);
    let ray = Ray::new(ray.origin.as_dvec3(), ray.direction.as_dvec3());
    let distance = (value - ray.origin[axis]) / ray.dir[axis];
    ray.origin + ray.dir * distance
}

/// The bottom centre of `id`'s box: where a dropped mesh's foot stands.
fn foot(editor: &mut Editor<HeadlessShell>, id: SceneEntityId) -> DVec3 {
    let (min, max) = editor.document.bounds(id).expect("placed");
    let centre = ((min + max) * 0.5).as_dvec3();
    DVec3::new(centre.x, f64::from(min.y), centre.z)
}

/// **A drop on a surface stands the mesh on the surface**: dragged from its
/// browser row onto the triangle's face, the new triangle's foot is where the
/// ray through the release pixel strikes that face — not the ground — it is
/// selected, and one undo takes it back to the files as they were.
#[test]
fn a_drop_on_a_surface_stands_the_mesh_on_it_as_one_undo() {
    let mut editor = props_editor();
    let before = editor.document.files().expect("saves");
    let entities = editor.document.entity_count();
    let grab = row_of(&editor, TRIANGLE);
    let release = pixel_of(&editor, Vec3::new(10.5, 5.5, 0.0));
    drag(&mut editor, grab, release);

    let id = editor.document.primary().expect("the new mesh is selected");
    assert_eq!(editor.document.entity_count(), entities + 1);
    assert_eq!(
        editor.document.mesh(id).map(|mesh| mesh.asset.clone()),
        Some(TRIANGLE.to_owned())
    );
    let struck = crossing(&editor, release, 2, MIN_HALF_EXTENT);
    let stood = foot(&mut editor, id);
    assert!(
        (stood - struck).length() < 1e-3,
        "the mesh stands at {stood}, the face was struck at {struck}",
    );
    assert!(
        stood.y > 1.0,
        "the drop fell through to the ground: {stood}"
    );

    editor.act(&Action::Undo);
    assert_eq!(editor.document.entity_count(), entities);
    assert_eq!(editor.document.files().expect("saves"), before);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A drop on nothing stands the mesh on the ground plane**, where the ray
/// through the release pixel meets `y = 0`.
#[test]
fn a_drop_on_nothing_stands_the_mesh_on_the_ground_plane() {
    let mut editor = props_editor();
    editor
        .document
        .delete(&[SceneEntityId(0)])
        .expect("the ground slab is in the scene");
    editor.frame().expect("a frame");
    let release = pixel_of(&editor, Vec3::new(3.0, 0.0, 3.0));
    let grab = row_of(&editor, TRIANGLE);
    drag(&mut editor, grab, release);

    let id = editor.document.primary().expect("the new mesh is selected");
    let ground = crossing(&editor, release, 1, 0.0);
    let stood = foot(&mut editor, id);
    assert!(
        (stood - ground).length() < 1e-3,
        "the mesh stands at {stood}, the ray meets the ground at {ground}",
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A drop released outside the viewport places nothing** — just under the
/// pane, where the ray through the release pixel would still meet the
/// ground — and a press in the viewport drags no asset.
#[test]
fn a_drop_outside_the_viewport_places_nothing() {
    let mut editor = props_editor();
    let entities = editor.document.entity_count();
    let grab = row_of(&editor, TRIANGLE);
    let (min, max) = editor.panels.viewport_pixels();
    let under = Vec2::new((min.x + max.x) * 0.5, max.y + 4.0);
    assert!(!editor.panels.in_viewport(under));
    assert!(
        Document::ground_point(&editor.ray_at(under)).is_ok(),
        "the release pixel's ray misses the ground, so a drop there proves nothing",
    );
    drag(&mut editor, grab, under);
    assert_eq!(editor.document.entity_count(), entities);
    let inside = pixel_of(&editor, Vec3::new(3.0, 0.0, 3.0));
    drag(&mut editor, inside, inside + Vec2::new(6.0, 0.0));
    assert_eq!(editor.document.entity_count(), entities);
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **A drop in play mode is refused on the status line**, placing nothing.
#[test]
fn a_drop_in_play_mode_is_refused() {
    let mut editor = props_editor();
    editor.document.play().expect("nothing here refuses play");
    let entities = editor.document.entity_count();
    let grab = row_of(&editor, TRIANGLE);
    let release = pixel_of(&editor, Vec3::new(3.0, 0.0, 3.0));
    drag(&mut editor, grab, release);
    assert_eq!(editor.document.entity_count(), entities);
    let (status, tone) = editor.panels.status();
    assert_eq!(tone, Tone::Warning);
    assert!(status.contains("play mode"), "{status}");
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}

/// **Enter on a browser row places its asset where the view's centre meets
/// the ground**, selected, as one undo.
#[test]
fn enter_on_a_row_places_the_asset_at_the_views_centre() {
    let mut editor = props_editor();
    let entities = editor.document.entity_count();
    let row = row_of(&editor, TRIANGLE);
    click(&mut editor, physical(row));
    assert_eq!(
        editor.document.entity_count(),
        entities,
        "a click on a row placed something",
    );
    tap(&mut editor, KeyCode::Enter);

    let id = editor.document.primary().expect("the new mesh is selected");
    assert_eq!(editor.document.entity_count(), entities + 1);
    let (min, max) = editor.panels.viewport_pixels();
    let ground = crossing(&editor, (min + max) * 0.5, 1, 0.0);
    let stood = foot(&mut editor, id);
    assert!(
        (stood - ground).length() < 1e-3,
        "the mesh stands at {stood}, the view's centre meets the ground at {ground}",
    );
    let (status, _) = editor.panels.status();
    assert!(status.contains(TRIANGLE), "{status}");

    editor.document.undo().expect("the placement undoes");
    assert_eq!(editor.document.entity_count(), entities);
    assert!(
        editor.document.systems_of(id).is_empty(),
        "the undone mesh is still in {MESHES}"
    );
    editor.finish(ExitReason::FrameBudget).expect("teardown");
}
