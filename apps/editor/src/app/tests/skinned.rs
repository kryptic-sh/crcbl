//! A rigged asset in the viewport: drawn through the skinning pass from one
//! posed region its every entity shares, at the rest pose.

use std::path::Path;

use super::*;

use super::scene_inspector::scrolled_to;

use crcbl::assets::MemorySource;
use crcbl::math::DVec3;
use crcbl_scene::gltf_fixture::{
    BIN_CHUNK_BUFFER, glb, rigged_bin, rigged_json, triangle_bin, triangle_json,
};

use crate::app::instances::Desc;
use crate::document::mesh_tests::{TRIANGLE, props};

const RIG: &str = "props/rig.glb";

/// The props' assets with a rig beside the triangle.
fn assets() -> MemorySource {
    let mut source = MemorySource::new();
    for (key, bytes) in [
        (
            RIG,
            glb(&rigged_json(BIN_CHUNK_BUFFER), Some(&rigged_bin())),
        ),
        (
            TRIANGLE,
            glb(&triangle_json(BIN_CHUNK_BUFFER), Some(&triangle_bin())),
        ),
    ] {
        source.insert(Path::new(key), bytes).expect("a legal key");
    }
    source
}

/// The descriptions the entity spawned as `id` is drawn with.
fn descs_of(editor: &Editor<HeadlessShell>, id: SceneEntityId) -> Vec<Desc> {
    editor
        .instances
        .instances
        .iter()
        .find(|placed| placed.drawn == instances::Drawn::Scene(id))
        .expect("the entity is placed")
        .descs()
        .to_vec()
}

/// **A rigged mesh is drawn from its posed region, and a second one shares
/// it.** Spawning the rig rebuilds the renderer around it once, with a skinning
/// pass; both entities' instances name the one region, each under its own
/// placement, and frames run through the skinned entry points. The triangle
/// beside them stays a plain instance.
#[test]
fn a_rigged_mesh_draws_from_one_shared_region() {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    editor.frame().expect("a frame");
    assert!(editor.skins.skinning().is_none(), "nothing rigged yet");

    let first = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor
        .frame()
        .expect("a frame through the skinned entry points");
    assert!(editor.skins.skinning().is_some(), "the rig built a pass");
    let draw = editor
        .skins
        .draw_of(RIG, 0)
        .expect("the rig's part is posed");

    let second = editor
        .document
        .spawn_mesh(RIG, DVec3::new(-2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    assert_eq!(editor.skins.draw_of(RIG, 0), Some(draw), "no second region");

    let skinned = |id| match descs_of(&editor, id).as_slice() {
        [
            Desc::Skinned {
                draw: named,
                transform,
                ..
            },
        ] => {
            assert_eq!(*named, draw, "both share the region");
            *transform
        }
        other => panic!("a rigged mesh drawn as {other:?}"),
    };
    let (a, b) = (skinned(first), skinned(second));
    assert!(
        (a.w_axis.x - b.w_axis.x - 4.0).abs() < 1e-5,
        "each under its own placement: {a} vs {b}"
    );
    let triangle = editor
        .document
        .outline()
        .into_iter()
        .flat_map(|(_, ids)| ids)
        .find(|&id| {
            editor
                .document
                .mesh(id)
                .is_some_and(|mesh| mesh.asset == TRIANGLE)
        })
        .expect("the props' triangle");
    assert!(matches!(
        descs_of(&editor, triangle).as_slice(),
        [Desc::Plain(_)]
    ));
    // Each frame begun through the skinned entry point flips the pass's half;
    // a frame that took the plain one would leave it where it was.
    let parity = |editor: &Editor<HeadlessShell>| editor.skins.skinning().expect("a pass").parity();
    let before = parity(&editor);
    editor.frame().expect("another skinned frame");
    assert_ne!(
        parity(&editor),
        before,
        "the frame skipped the skinning pass"
    );
}

/// **Selecting a rigged mesh shows its skeleton**, and selecting the triangle
/// or nothing shows none.
#[test]
fn a_selected_rigged_mesh_shows_its_bones() {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let rig = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    editor.document.select(None);
    assert!(selected_bones(&mut editor.document, &editor.shelf).is_empty());
    editor.document.select(Some(rig));
    let bones = selected_bones(&mut editor.document, &editor.shelf);
    assert_eq!(bones.len(), 1, "the rig's one bone: {bones:?}");
    assert!(
        (bones[0].1.y - bones[0].0.y - 1.0).abs() < 1e-5,
        "a unit long, upwards"
    );
}

/// **Selecting a rigged mesh shows the ragdoll rig fitted to it**: the
/// fixture's one bone carries a capsule along it and its tip a sphere, both
/// standing where the mesh does.
#[test]
fn a_selected_rigged_mesh_shows_its_fitted_ragdoll() {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let rig = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    editor.document.select(Some(rig));
    let bodies = selected_ragdoll_bodies(&mut editor.document, &editor.shelf);
    let [(capsule, along), (sphere, tip)] = bodies.as_slice() else {
        panic!("a capsule and a sphere: {bodies:?}");
    };
    assert!(matches!(capsule, crcbl::ragdoll::BodyShape::Capsule { .. }));
    assert!(matches!(sphere, crcbl::ragdoll::BodyShape::Sphere { .. }));
    // Where the mesh stands is where its bone does: the capsule is centred
    // on the bone, and the sphere sits past its tip.
    let [(root, tip_joint)] = selected_bones(&mut editor.document, &editor.shelf)[..] else {
        panic!("one bone");
    };
    let middle = ((root + tip_joint) * 0.5).as_dvec3();
    assert!(
        along.position.distance(middle) < 1e-5,
        "{along:?} vs {middle}"
    );
    assert!(
        tip.position.y > 1.0,
        "the sphere sits past the tip: {tip:?}"
    );
    editor.document.select(None);
    assert!(selected_ragdoll_bodies(&mut editor.document, &editor.shelf).is_empty());
}

/// **A box body's corners are in the order `box_edges` joins**: corners whose
/// indices differ in one bit differ along exactly one of the box's own axes,
/// however the box is turned.
#[test]
fn box_corners_follow_the_edge_order() {
    let half = DVec3::new(0.5, 1.0, 2.0);
    let at = crcbl::phys::Transform::new(
        DVec3::new(1.0, 2.0, 3.0),
        crcbl::math::DQuat::from_rotation_y(0.7) * crcbl::math::DQuat::from_rotation_x(0.3),
    );
    let corners = box_corners(half, &at);
    for i in 0..8_usize {
        for bit in [1_usize, 2, 4] {
            let j = i ^ bit;
            let length = corners[i].distance(corners[j]);
            let want = match bit {
                1 => 2.0 * half.x,
                2 => 2.0 * half.y,
                _ => 2.0 * half.z,
            };
            assert!(
                (f64::from(length) - want).abs() < 1e-5,
                "corners {i} and {j}: {length} vs {want}"
            );
        }
    }
}

/// **G drops the selected rigged mesh as a ragdoll, and G stops it.** The
/// dropped entity draws from its preview region while the bodies fall; its
/// twin of the same asset stays on the shared rest-pose region; and after G
/// the dropped one is back on it too. Nothing in the scene changes.
#[test]
fn g_drops_a_selected_rigged_mesh_and_g_stops_it() {
    let mut editor = headless(256);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let dropped = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 3.0, 0.0))
        .expect("spawned");
    let twin = editor
        .document
        .spawn_mesh(RIG, DVec3::new(-2.0, 3.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    let before = editor.document.files().expect("files");
    editor.document.select(Some(dropped));
    let shared = editor.skins.draw_of(RIG, 0).expect("a shared region");
    let preview = editor.skins.preview_of(RIG, 0).expect("a preview region");
    assert_ne!(shared, preview);

    tap(&mut editor, KeyCode::KeyG);
    let start = editor.dropping.as_ref().expect("dropping").heights();
    for _ in 0..30 {
        editor.frame().expect("a frame");
    }
    let fallen = editor.dropping.as_ref().expect("still dropping").heights();
    assert!(
        fallen.iter().zip(&start).all(|(now, then)| now < then),
        "the bodies did not fall: {start:?} to {fallen:?}"
    );
    let drawn_from = |editor: &Editor<HeadlessShell>, id| match descs_of(editor, id).as_slice() {
        [Desc::Skinned { draw, .. }] => *draw,
        other => panic!("a rigged mesh drawn as {other:?}"),
    };
    assert_eq!(drawn_from(&editor, dropped), preview);
    assert_eq!(drawn_from(&editor, twin), shared);
    assert_ne!(
        editor.skins.palette(preview),
        editor.skins.palette(shared),
        "the preview region still holds the rest pose"
    );

    tap(&mut editor, KeyCode::KeyG);
    assert!(editor.dropping.is_none());
    assert_eq!(drawn_from(&editor, dropped), shared);
    assert_eq!(
        editor.document.files().expect("files"),
        before,
        "the scene changed"
    );
}

/// Where the lowest of the bodies of a rig dropped over a twin of it stands
/// once it has fallen, with the twin given a body row (`solid`) or not, and
/// how tall the twin's box is.
fn lowest_after_dropping_over_a_twin(solid: bool) -> (f64, f64) {
    let mut editor = headless(64);
    editor.document = props();
    editor.document.set_assets(Box::new(assets()));
    let twin = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    let placed = editor.document.placement(twin).expect("the twin is placed");
    let dropped = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 2.0 * placed.half_extents.y, 0.0))
        .expect("spawned");
    if solid {
        editor
            .document
            .attach(twin, crcbl::scene_physics::BODIES)
            .expect("a body row");
    }
    editor.frame().expect("a frame");
    editor.document.select(Some(dropped));
    tap(&mut editor, KeyCode::KeyG);
    for _ in 0..90 {
        editor.frame().expect("a frame");
    }
    let lowest = editor
        .dropping
        .as_ref()
        .expect("still dropping")
        .heights()
        .into_iter()
        .fold(f64::INFINITY, f64::min);
    (lowest, 2.0 * placed.half_extents.y)
}

/// **The drop lands on what the scene collides as**: over a twin with a body
/// row it comes to rest on the twin's box, well above where it falls to with
/// the row gone, when only the ground is under it.
#[test]
fn the_drop_lands_on_an_entity_with_a_body_row() {
    let (on_ground, _) = lowest_after_dropping_over_a_twin(false);
    let (on_twin, height) = lowest_after_dropping_over_a_twin(true);
    assert!(
        on_twin - on_ground > 0.5 * height,
        "the bodies rest at {on_twin} m on a {height} m box and at {on_ground} m without it"
    );
}

/// Puts the pointer over the inspector, so the wheel scrolls it: the wheel
/// scrolls the pane under the pointer.
fn hover_the_inspector(editor: &mut Editor<HeadlessShell>) {
    let pane = editor.panels.props_key().expect("the inspector was built");
    let over = centre(editor, pane);
    let window = editor.window;
    editor
        .shell_mut()
        .move_pointer(window, over, (0.0, 0.0))
        .expect("live");
    editor.frame().expect("a frame");
}

/// An editor whose assets are a directory holding the rig, with the rig
/// spawned, drawn once and selected.
fn rig_on_disk() -> (tempfile::TempDir, Editor<HeadlessShell>) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::create_dir_all(dir.path().join("props")).expect("a props folder");
    std::fs::write(
        dir.path().join(RIG),
        glb(&rigged_json(BIN_CHUNK_BUFFER), Some(&rigged_bin())),
    )
    .expect("the rig asset");
    let mut editor = headless(64);
    editor.document = props();
    editor
        .document
        .set_assets(Box::new(crcbl::assets::DirSource::at(
            dir.path().to_path_buf(),
        )));
    editor.assets = Some(dir.path().to_path_buf());
    let rig = editor
        .document
        .spawn_mesh(RIG, DVec3::new(2.0, 0.0, 0.0))
        .expect("spawned");
    editor.frame().expect("a frame");
    editor.document.select(Some(rig));
    (dir, editor)
}

/// **Shift+G writes the fitted rig beside the asset as a file to author
/// from, and never overwrites one.** The file reads back as a rig for the
/// asset and the shelf takes it up as the authored rig; it lists no pairs and
/// leaves them to the rule for rest overlaps, as the fitter did. A second
/// press finds the file there and leaves it — even one a person has since
/// edited.
#[test]
fn shift_g_writes_the_rig_once() {
    let (dir, mut editor) = rig_on_disk();

    editor.act(&Action::WriteRig);
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));
    let written = std::fs::read_to_string(&path).expect("the rig was written");
    assert!(
        editor.panels.status().0.starts_with("Wrote"),
        "{:?}",
        editor.panels.status()
    );
    let file = crcbl::ragdoll::RagdollDesc::from_ron(&written).expect("a rig file");
    assert!(file.ignore_rest_overlaps, "the pairs are frozen: {written}");
    assert!(file.ignore.is_empty(), "the pairs are frozen: {written}");
    editor.frame().expect("a rebuild reading it back");
    assert!(
        editor
            .shelf
            .rig(RIG)
            .and_then(|rig| rig.ragdoll.as_ref())
            .is_some(),
        "the shelf reads the written rig"
    );

    std::fs::write(&path, "an edit a person made").expect("an edit");
    editor.act(&Action::WriteRig);
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "an edit a person made",
        "an authored rig was overwritten"
    );
    assert!(editor.panels.status().0.contains("already authored"));
}

/// The node under `at` holding the span that reads `text`, searched depth
/// first: a scalar row for its label, a group's header for its title.
fn holder_of(ui: &crcbl::ui::tree::Ui, at: NodeKey, text: &str) -> Option<NodeKey> {
    let children = ui.child_keys(at);
    if children.iter().any(|&child| ui.text(child) == Some(text)) {
        return Some(at);
    }
    children
        .into_iter()
        .find_map(|child| holder_of(ui, child, text))
}

/// The widget of the row labelled `label` under `rows`: a scalar row is its
/// label span and then its widget.
fn widget_labelled(editor: &Editor<HeadlessShell>, rows: NodeKey, label: &str) -> NodeKey {
    let ui = editor.panels.ui();
    let row = holder_of(ui, rows, label).unwrap_or_else(|| panic!("no row labelled {label:?}"));
    ui.child_keys(row)[1]
}

/// The selected rig's first body's mass, as the inspector's rows edit it and
/// as the rig drawn and dropped holds it.
fn first_mass(editor: &Editor<HeadlessShell>) -> (f64, f64) {
    let rig = editor.shelf.rig(RIG).expect("the rig");
    let edited = rig.desc.as_ref().expect("a rig file").bodies[0].mass;
    let drawn = rig.ragdoll.as_ref().expect("a rig").bodies()[0].mass;
    (edited, drawn)
}

/// **Dragging a body's mass in the inspector changes the rig at once and
/// writes its file when the button lets go.** While the button is held the
/// rig drawn and dropped already has the new mass and no file is written;
/// the release writes `<asset>.ragdoll.ron` once, holding it.
#[test]
fn dragging_a_rig_row_changes_the_rig_and_writes_it_on_release() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    let (before, drawn) = first_mass(&editor);
    assert_eq!(drawn, before);
    hover_the_inspector(&mut editor);
    let at = scrolled_to(&mut editor, |editor| {
        let rows = editor
            .panels
            .rig_body_fields(0)
            .expect("the inspector drew the rig's bodies");
        widget_labelled(editor, rows, "Mass (kg)")
    });
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));

    let window = editor.window;
    let shell = editor.shell_mut();
    shell.move_pointer(window, at, (0.0, 0.0)).expect("live");
    shell
        .button(window, PointerButton::Left, ButtonState::Pressed, Some(at))
        .expect("live");
    editor.frame().expect("a frame");
    for step in 1..=4 {
        let to = PhysicalPoint {
            x: at.x + 10.0 * f64::from(step),
            y: at.y,
        };
        editor
            .shell_mut()
            .move_pointer(window, to, (10.0, 0.0))
            .expect("live");
        editor.frame().expect("a frame");
    }
    let (held, drawn) = first_mass(&editor);
    assert!(
        held > before,
        "the drag moved no mass: {held} from {before}"
    );
    assert_eq!(drawn, held, "the rig drawn is not the rig edited");
    assert!(!path.exists(), "the file was written before the release");

    let end = PhysicalPoint {
        x: at.x + 40.0,
        y: at.y,
    };
    editor
        .shell_mut()
        .button(
            window,
            PointerButton::Left,
            ButtonState::Released,
            Some(end),
        )
        .expect("live");
    editor.frame().expect("a frame");
    let written = std::fs::read_to_string(&path).expect("the release wrote the rig");
    let file = crcbl::ragdoll::RagdollDesc::from_ron(&written).expect("a rig file");
    assert_eq!(file.bodies[0].mass, held);
    assert!(
        editor.panels.status().0.starts_with("Wrote"),
        "{:?}",
        editor.panels.status()
    );
}

/// Drags the widget at `at` across by each of `moves` in pixels, a frame
/// each, and lets go.
fn drag_through(editor: &mut Editor<HeadlessShell>, at: PhysicalPoint, moves: &[f64]) {
    let window = editor.window;
    let shell = editor.shell_mut();
    shell.move_pointer(window, at, (0.0, 0.0)).expect("live");
    shell
        .button(window, PointerButton::Left, ButtonState::Pressed, Some(at))
        .expect("live");
    editor.frame().expect("a frame");
    let mut to = at;
    for &by in moves {
        to.x += by;
        editor
            .shell_mut()
            .move_pointer(window, to, (by, 0.0))
            .expect("live");
        editor.frame().expect("a frame");
    }
    editor
        .shell_mut()
        .button(window, PointerButton::Left, ButtonState::Released, Some(to))
        .expect("live");
    editor.frame().expect("a frame");
}

/// **A joint's limit dragged past the other is refused, by name, and
/// nothing is written**: the status line says why, the rig drawn and dropped
/// keeps the last limits it could take, and no file appears — not even the
/// step it took before — since a rig file always resolves.
#[test]
fn a_limit_dragged_past_the_other_is_refused_and_not_written() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    let rig = editor.shelf.rig(RIG).expect("the rig");
    let joints = rig.ragdoll.as_ref().expect("a rig").joints().to_vec();
    let (group, label) = match rig.desc.as_ref().expect("a rig file").joints[0].kind {
        crcbl::ragdoll::JointKindDesc::Ball { .. } => ("Kind: Ball", "Least twist (°)"),
        crcbl::ragdoll::JointKindDesc::Hinge { .. } => ("Kind: Hinge", "Least angle (°)"),
        crcbl::ragdoll::JointKindDesc::Fixed => panic!("the fitted joint has limits"),
    };
    let joint = |editor: &Editor<HeadlessShell>| {
        editor
            .panels
            .rig_joint_fields(0)
            .expect("the inspector drew the rig's joints")
    };
    // The wheel scrolls the pane under the pointer.
    let pane = editor.panels.props_key().expect("the inspector was built");
    let over = centre(&editor, pane);
    let window = editor.window;
    editor
        .shell_mut()
        .move_pointer(window, over, (0.0, 0.0))
        .expect("live");
    editor.frame().expect("a frame");
    let at = scrolled_to(&mut editor, |editor| {
        holder_of(editor.panels.ui(), joint(editor), group).expect("the kind's group")
    });
    click(&mut editor, at);
    let at = scrolled_to(&mut editor, |editor| {
        widget_labelled(editor, joint(editor), label)
    });

    // A step the rig can take, then one past the other limit: the change it
    // took is not written under the one it refused.
    drag_through(&mut editor, at, &[12.0, 720.0]);
    let status = editor.panels.status();
    assert!(status.0.contains("cannot take that"), "{status:?}");
    let rig = editor.shelf.rig(RIG).expect("the rig");
    assert_ne!(
        rig.ragdoll.as_ref().expect("a rig").joints(),
        joints,
        "the first step was taken"
    );
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));
    assert!(!path.exists(), "a rig that does not resolve was written");
}

/// **A body's shape picked from its drop-down keeps the body's size**: the
/// fitted capsule turned into a sphere has the capsule's radius, the rig drawn
/// and dropped is the sphere, and the file is written with it.
#[test]
fn a_shape_picked_from_the_drop_down_keeps_its_size() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    let crcbl::ragdoll::ShapeDesc::Capsule { radius, .. } = editor
        .shelf
        .rig(RIG)
        .expect("the rig")
        .desc
        .as_ref()
        .expect("a rig file")
        .bodies[0]
        .shape
    else {
        panic!("the fitted body is a capsule");
    };
    let body = |editor: &Editor<HeadlessShell>| {
        editor
            .panels
            .rig_body_fields(0)
            .expect("the inspector drew the rig's bodies")
    };
    let pick = |editor: &mut Editor<HeadlessShell>,
                find: &dyn Fn(&Editor<HeadlessShell>) -> NodeKey| {
        let at = centre(editor, find(editor));
        click(editor, at);
    };
    hover_the_inspector(&mut editor);
    scrolled_to(&mut editor, |editor| body(editor));
    pick(&mut editor, &|editor| {
        holder_of(editor.panels.ui(), body(editor), "Shape: Capsule").expect("the shape's group")
    });
    let select = holder_of(editor.panels.ui(), body(&editor), "Capsule").expect("its drop-down");
    pick(&mut editor, &|_| select);
    pick(&mut editor, &|editor| {
        holder_of(
            editor.panels.ui(),
            crcbl::ui::tree::Ui::popup_key(select),
            "Sphere",
        )
        .expect("the drop-down lists the sphere")
    });

    let rig = editor.shelf.rig(RIG).expect("the rig");
    let sphere = crcbl::ragdoll::ShapeDesc::Sphere { radius };
    assert_eq!(
        rig.desc.as_ref().expect("a rig file").bodies[0].shape,
        sphere
    );
    assert_eq!(
        rig.ragdoll.as_ref().expect("a rig").bodies()[0].shape,
        crcbl::ragdoll::BodyShape::Sphere { radius },
        "the rig drawn is not the sphere"
    );
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));
    let written = std::fs::read_to_string(&path).expect("the pick wrote the rig");
    let file = crcbl::ragdoll::RagdollDesc::from_ron(&written).expect("a rig file");
    assert_eq!(file.bodies[0].shape, sphere);
}

/// **The rig's own rows switch the rule for rest overlaps and set the
/// substeps**: each edit is taken by the rig drawn and dropped and written to
/// the file.
#[test]
fn the_rig_rows_set_the_overlap_rule_and_the_substeps() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    let rows = |editor: &Editor<HeadlessShell>| {
        editor
            .panels
            .rig_fields()
            .expect("the inspector drew the rig's rows")
    };
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));
    let file = || {
        let written = std::fs::read_to_string(&path).expect("the rig was written");
        crcbl::ragdoll::RagdollDesc::from_ron(&written).expect("a rig file")
    };

    let at = centre(
        &editor,
        widget_labelled(&editor, rows(&editor), "Ignore rest overlaps"),
    );
    click(&mut editor, at);
    let rig = editor.shelf.rig(RIG).expect("the rig");
    assert!(!rig.desc.as_ref().expect("a rig file").ignore_rest_overlaps);
    assert!(!file().ignore_rest_overlaps, "the switch was not written");

    let fitted = rig.desc.as_ref().expect("a rig file").substeps;
    let at = centre(&editor, widget_labelled(&editor, rows(&editor), "Substeps"));
    drag_through(&mut editor, at, &[12.0, 40.0]);
    let rig = editor.shelf.rig(RIG).expect("the rig");
    let substeps = rig.desc.as_ref().expect("a rig file").substeps;
    assert!(substeps > fitted, "the drag set no substeps: {substeps}");
    assert_eq!(rig.ragdoll.as_ref().expect("a rig").substeps(), substeps);
    assert_eq!(file().substeps, substeps, "the substeps were not written");
}

/// The drag-value of component `axis` (0 for x) of the three-number row
/// labelled `label` under `rows`: a vector row is its label span and then a
/// block per component, each its axis label and its widget.
fn component_labelled(
    editor: &Editor<HeadlessShell>,
    rows: NodeKey,
    label: &str,
    axis: usize,
) -> NodeKey {
    let ui = editor.panels.ui();
    let row = holder_of(ui, rows, label).unwrap_or_else(|| panic!("no row labelled {label:?}"));
    let component = ui.child_keys(row)[1 + axis];
    ui.child_keys(component)[1]
}

/// **Where a body sits and which way it points, and a joint's axis and
/// offset, are rows**: dragging each one's x moves that component of the rig
/// file alone, the rig drawn and dropped follows it, and the file is written
/// holding it.
#[test]
fn placement_rows_move_the_bodies_and_joints() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    let body = |editor: &Editor<HeadlessShell>| {
        editor
            .panels
            .rig_body_fields(0)
            .expect("the inspector drew the rig's bodies")
    };
    let joint = |editor: &Editor<HeadlessShell>| {
        editor
            .panels
            .rig_joint_fields(0)
            .expect("the inspector drew the rig's joints")
    };
    let desc = |editor: &Editor<HeadlessShell>| {
        editor
            .shelf
            .rig(RIG)
            .and_then(|rig| rig.desc.clone())
            .expect("a rig file")
    };
    let drawn = |editor: &Editor<HeadlessShell>| {
        editor
            .shelf
            .rig(RIG)
            .and_then(|rig| rig.ragdoll.clone())
            .expect("a rig")
    };
    let drag = |editor: &mut Editor<HeadlessShell>,
                rows: &dyn Fn(&Editor<HeadlessShell>) -> NodeKey,
                label: &str| {
        // The last drag left the pointer wherever it let go.
        hover_the_inspector(editor);
        let at = scrolled_to(editor, |editor| {
            component_labelled(editor, rows(editor), label, 0)
        });
        drag_through(editor, at, &[12.0, 30.0]);
    };

    let (before, rig) = (desc(&editor), drawn(&editor));
    drag(&mut editor, &body, "Centre (m)");
    let (after, moved) = (desc(&editor), drawn(&editor));
    let (from, to) = (before.bodies[0].centre, after.bodies[0].centre);
    assert!(to.0 > from.0, "the drag moved no centre: {to:?}");
    assert_eq!((to.1, to.2), (from.1, from.2), "only x moves");
    assert_ne!(moved.bodies()[0].offset, rig.bodies()[0].offset);

    let (before, rig) = (after, moved);
    drag(&mut editor, &body, "Axis");
    let (after, moved) = (desc(&editor), drawn(&editor));
    let (from, to) = (before.bodies[0].axis, after.bodies[0].axis);
    assert!(to.0 > from.0, "the drag turned no axis: {to:?}");
    assert_eq!((to.1, to.2), (from.1, from.2), "only x moves");
    assert_ne!(
        moved.bodies()[0].offset.rotation,
        rig.bodies()[0].offset.rotation
    );

    let (before, rig) = (after, moved);
    drag(&mut editor, &joint, "Offset (m)");
    let (after, moved) = (desc(&editor), drawn(&editor));
    let (from, to) = (before.joints[0].at, after.joints[0].at);
    assert!(to.0 > from.0, "the drag moved no joint: {to:?}");
    assert_eq!((to.1, to.2), (from.1, from.2), "only x moves");
    assert_ne!(
        moved.joints()[0].frame_child.position,
        rig.joints()[0].frame_child.position
    );

    let (before, rig) = (after, moved);
    drag(&mut editor, &joint, "Axis");
    let (after, moved) = (desc(&editor), drawn(&editor));
    let (from, to) = (before.joints[0].axis, after.joints[0].axis);
    assert!(to.0 > from.0, "the drag turned no joint axis: {to:?}");
    assert_eq!((to.1, to.2), (from.1, from.2), "only x moves");
    assert_ne!(
        moved.joints()[0].frame_child.rotation,
        rig.joints()[0].frame_child.rotation
    );

    let written = std::fs::read_to_string(dir.path().join(format!("{RIG}.ragdoll.ron")))
        .expect("the rig was written");
    let file = crcbl::ragdoll::RagdollDesc::from_ron(&written).expect("a rig file");
    assert_eq!(file, after, "the file is not the rig edited");
}

/// **The rig section's Undo and Redo step the rig's own history and write
/// it**: a written drag is one step, Undo puts the mass back in the rig drawn
/// and in the file, Redo puts the edit back, and each button is there only
/// with a step to take.
#[test]
fn the_rig_section_undoes_and_redoes_its_edits() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    assert_eq!(
        editor.panels.rig_history_buttons(),
        (None, None),
        "nothing to step yet"
    );
    let (before, _) = first_mass(&editor);
    hover_the_inspector(&mut editor);
    let at = scrolled_to(&mut editor, |editor| {
        let rows = editor
            .panels
            .rig_body_fields(0)
            .expect("the inspector drew the rig's bodies");
        widget_labelled(editor, rows, "Mass (kg)")
    });
    drag_through(&mut editor, at, &[12.0, 30.0]);
    // The release wrote the rig after the panels were built: the next frame
    // draws the step it made.
    editor.frame().expect("a frame");
    let (edited, _) = first_mass(&editor);
    assert!(edited > before, "the drag moved no mass");
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));
    let file_mass = || {
        let written = std::fs::read_to_string(&path).expect("the rig was written");
        crcbl::ragdoll::RagdollDesc::from_ron(&written)
            .expect("a rig file")
            .bodies[0]
            .mass
    };

    let (undo, redo) = editor.panels.rig_history_buttons();
    assert!(redo.is_none(), "nothing undone yet");
    let undo = undo.expect("a step back");
    hover_the_inspector(&mut editor);
    let at = scrolled_to(&mut editor, |_| undo);
    click(&mut editor, at);
    assert_eq!(first_mass(&editor), (before, before), "undone in the rig");
    assert_eq!(file_mass(), before, "undone in the file");
    assert!(editor.panels.status().0.starts_with("Undid"));

    let (undo, redo) = editor.panels.rig_history_buttons();
    assert!(undo.is_none(), "the one step is taken");
    let redo = redo.expect("a step forward");
    hover_the_inspector(&mut editor);
    let at = scrolled_to(&mut editor, |_| redo);
    click(&mut editor, at);
    assert_eq!(first_mass(&editor), (edited, edited), "redone in the rig");
    assert_eq!(file_mass(), edited, "redone in the file");
}

/// **A pair is listed from the drop-down and unlisted by its button**: the
/// fitted file lists none; picking the first two bodies' pair lists it in the
/// rig drawn and the file, and its Remove button takes it out of both.
#[test]
fn rig_pairs_are_listed_and_removed() {
    let (dir, mut editor) = rig_on_disk();
    editor.frame().expect("the inspector draws the rig");
    let desc = |editor: &Editor<HeadlessShell>| {
        editor
            .shelf
            .rig(RIG)
            .expect("the rig")
            .desc
            .clone()
            .expect("a rig file")
    };
    let fitted = desc(&editor);
    assert!(fitted.ignore.is_empty(), "the fitted file lists pairs");
    let pair = (fitted.bodies[0].bone.clone(), fitted.bodies[1].bone.clone());
    let path = dir.path().join(format!("{RIG}.ragdoll.ron"));
    let file = || {
        let written = std::fs::read_to_string(&path).expect("the rig was written");
        crcbl::ragdoll::RagdollDesc::from_ron(&written)
            .expect("a rig file")
            .ignore
    };
    let (removes, select) = editor.panels.rig_pairs();
    assert!(removes.is_empty(), "a line for a pair not listed");
    let select = select.expect("the drop-down of pairs");
    hover_the_inspector(&mut editor);

    let at = scrolled_to(&mut editor, |_| select);
    click(&mut editor, at);
    let label = format!("{} ↔ {}", pair.0, pair.1);
    let option = holder_of(
        editor.panels.ui(),
        crcbl::ui::tree::Ui::popup_key(select),
        &label,
    )
    .expect("the drop-down offers the pair");
    let at = centre(&editor, option);
    click(&mut editor, at);
    assert_eq!(
        desc(&editor).ignore,
        std::slice::from_ref(&pair),
        "listed in the rig"
    );
    assert_eq!(file(), [pair], "listed in the file");

    // The frame also draws the listed pair's line.
    hover_the_inspector(&mut editor);
    let remove = |editor: &Editor<HeadlessShell>| {
        *editor
            .panels
            .rig_pairs()
            .0
            .first()
            .expect("a line for the listed pair")
    };
    let at = scrolled_to(&mut editor, remove);
    click(&mut editor, at);
    assert!(desc(&editor).ignore.is_empty(), "still listed in the rig");
    assert!(file().is_empty(), "still listed in the file");
}
