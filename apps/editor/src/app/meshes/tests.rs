//! The shelf keeps an asset's rig, and nothing for an asset with no skin.

use std::collections::BTreeSet;
use std::path::Path;

use crcbl::assets::MemorySource;
use crcbl_scene::gltf_fixture::{
    BIN_CHUNK_BUFFER, glb, rigged_bin, rigged_json, triangle_bin, triangle_json,
};

use super::Shelf;

const RIG: &str = "props/rig.glb";
const TRIANGLE: &str = "props/triangle.glb";

fn source() -> MemorySource {
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

/// **A rigged asset keeps its skeleton and the part its skin deforms**, with a
/// binding for every vertex of that part; a plain one keeps no rig.
#[test]
fn a_rigged_asset_keeps_its_rig() {
    let assets: BTreeSet<String> = [RIG, TRIANGLE].map(str::to_owned).into();
    let (shelf, _) = Shelf::build(&source(), &assets);
    let rig = shelf.rig(RIG).expect("the rigged asset kept a rig");
    assert_eq!(rig.skeleton.len(), 2, "two joints");
    assert_eq!(rig.skinned.joints, 2);
    let parts = shelf.parts(RIG).expect("on the shelf");
    let [deformed] = rig.skinned.instances.as_slice() else {
        panic!("one skinned part: {:?}", rig.skinned.instances);
    };
    assert!(
        deformed.instance < parts.len(),
        "an index into the asset's parts"
    );
    // `rigged_json`'s one skinned primitive is a triangle.
    assert_eq!(deformed.bindings.len(), 3, "a binding per vertex");
    assert!(shelf.rig(TRIANGLE).is_none(), "a triangle has no skin");
}

/// **The scene reserves the posed region's room**: two mesh entries and two
/// vertex runs per deformed part beyond what the conversion itself sizes,
/// which a skinned draw takes and a pool sized to the description alone
/// would refuse.
#[test]
fn a_rig_reserves_its_posed_region() {
    let only = |asset: &str| {
        let assets: BTreeSet<String> = [asset.to_owned()].into();
        Shelf::build(&source(), &assets).1.capacities
    };
    let (rigged, plain) = (only(RIG), only(TRIANGLE));
    let converted = |asset: &str| {
        let imported = crcbl::scene::import_gltf(&source(), Path::new(asset)).expect("imports");
        crcbl::scene::build_render_scene(&imported, Path::new(asset))
            .scene
            .capacities
    };
    let (rig, triangle) = (converted(RIG), converted(TRIANGLE));
    assert_eq!(
        rigged.meshes - rig.meshes,
        plain.meshes - triangle.meshes + 2,
        "two mesh entries for the one deformed part"
    );
    assert_eq!(
        rigged.vertices - rig.vertices,
        plain.vertices - triangle.vertices + 2 * 3,
        "two runs of the deformed triangle's vertices"
    );
}

/// **A rig's bones stand where its entity puts the asset.** `rigged_json`'s
/// root joint sits at the asset's origin and its tip one unit up, so the one
/// bone runs from the entity's origin to a unit above it.
#[test]
fn a_rigs_bones_stand_where_its_entity_puts_it() {
    let assets: BTreeSet<String> = [RIG.to_owned()].into();
    let (shelf, _) = Shelf::build(&source(), &assets);
    let rig = shelf.rig(RIG).expect("a rig");
    let origin = crcbl::math::Mat4::from_translation(crcbl::math::Vec3::new(2.0, 0.0, -1.0));
    let bones = rig.bones(origin);
    let [(from, to)] = bones.as_slice() else {
        panic!("one bone: {bones:?}");
    };
    assert!(
        from.distance(crcbl::math::Vec3::new(2.0, 0.0, -1.0)) < 1e-6,
        "{from}"
    );
    assert!(
        to.distance(crcbl::math::Vec3::new(2.0, 1.0, -1.0)) < 1e-6,
        "{to}"
    );
}
