//! The committed frames: one per [`GeometryPath`], at each end of the dolly.
//!
//! `docs/plan/sample/14-quarry.md`'s exit criteria ask for "golden frames per
//! `GeometryPath` from the fixed dolly". Everything else this suite asserts is a
//! **count** — coverage, the cut, the triangle totals — and
//! [`Frame::pixels_rgba`](crate::harness::Frame) exists because a face lit from
//! the wrong side leaves every one of them unchanged. This module is the half
//! that would notice.
//!
//! # Why six, and why these six
//!
//! Three paths at two dolly stops. The dolly **translates into the quarry**
//! rather than zooming: at the start the face is a lit ridge against the sky, at
//! the end the camera is inside it and the face fills the frame. Named for the
//! stop rather than for a distance, because "near" and "far" invert depending on
//! whether you mean the camera or the geometry — and they were written the wrong
//! way round the first time.
//!
//! One stop per path would catch a path that
//! breaks outright and miss one that breaks partway down the dolly, which is
//! where the cut changes and therefore where this sample's whole subject lives.
//! Nine stops per path would be twenty-seven images for one fixture.
//!
//! The budget is [`MIXING_BUDGET`] rather than the default, because that is
//! where the three paths genuinely disagree: the mesh path draws levels 1 and 2
//! per cluster while the other two select one level per instance. At the default
//! budget all three draw the base mesh and one golden would do for all of them —
//! a per-path golden that cannot tell the paths apart is a per-path golden in
//! name only.
//!
//! # The tolerance is measured, not guessed
//!
//! [`Tolerance::RASTERISER`](crcbl_golden::Tolerance::RASTERISER)'s numbers were
//! taken between radv and lavapipe, which are exactly the two drivers this
//! content has to survive: the frames are blessed on an RX 7900 XTX here and
//! compared on lavapipe in CI. `crcbl-golden`'s crate docs carry the
//! measurements.

use crcbl::hal::GeometryPath;
use crcbl::render::DirectionalLight;

use crate::harness::{DOLLY_END, DOLLY_START, EXTENT, Levels, MIXING_BUDGET, Quarry, backend};

/// The sun the goldens are lit by.
///
/// Fixed rather than defaulted so a change to the engine's default direction
/// shows up as a failing golden here — which is the point — rather than as six
/// images that quietly re-bless to something else.
fn sun() -> DirectionalLight {
    DirectionalLight {
        direction: crcbl::math::Vec3::new(1.0, 0.8, 0.6).normalize(),
        ..DirectionalLight::default()
    }
}

/// Renders one frame and checks it against its committed reference.
fn check(path: GeometryPath, at: f32, name: &str) {
    let mut quarry = Quarry::open_on(Levels::Dag, MIXING_BUDGET, path);
    let frame = quarry.lit_frame(at, &sun());
    quarry.finish();

    let (width, height) = EXTENT;
    let image = crcbl_golden::Image::from_rgba8(width, height, frame.pixels_rgba)
        .expect("the readback is one RGBA8 frame of the ring's extent");

    let reference = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.png"));
    let golden =
        crcbl_golden::Golden::new(reference).with_tolerance(crcbl_golden::Tolerance::RASTERISER);
    let outcome = golden.check(&image).expect("the reference is readable");
    let comparison = outcome
        .into_result()
        .unwrap_or_else(|message| panic!("{message}"));
    // Printed on success too: the numbers are how the tolerance stays honest
    // across two drivers, and a run that quietly passes teaches nothing.
    eprintln!("quarry goldens: {name} — {}", comparison.summary());
}

/// The guard every test below shares: the Null backend draws nothing.
fn drew_nothing(name: &str) -> bool {
    if backend() == crcbl::backend::GpuBackend::Null {
        eprintln!(
            "quarry goldens: {name} — the Null backend draws nothing, so there are no pixels to \
             compare; run with CRCBL_GPU=vk"
        );
        return true;
    }
    false
}

/// One test per image rather than one over six.
///
/// `crcbl-golden` **fails a blessing run on purpose**, so that re-blessing can
/// never be mistaken for comparing. A single test would therefore write one
/// image and stop, and blessing six would take six runs. One test per golden
/// blesses the set in one pass with `--no-fail-fast`, and names in the failure
/// which frame moved.
macro_rules! golden_test {
    ($fn_name:ident, $path:expr, $at:expr, $name:literal) => {
        #[test]
        fn $fn_name() {
            if drew_nothing($name) {
                return;
            }
            check($path, $at, $name);
        }
    };
}

golden_test!(
    the_mesh_shader_path_matches_its_golden_at_the_dolly_start,
    GeometryPath::MeshShader,
    DOLLY_START,
    "mesh-shader-dolly-start"
);
golden_test!(
    the_mesh_shader_path_matches_its_golden_at_the_dolly_end,
    GeometryPath::MeshShader,
    DOLLY_END,
    "mesh-shader-dolly-end"
);
golden_test!(
    the_indirect_count_path_matches_its_golden_at_the_dolly_start,
    GeometryPath::IndirectCount,
    DOLLY_START,
    "indirect-count-dolly-start"
);
golden_test!(
    the_indirect_count_path_matches_its_golden_at_the_dolly_end,
    GeometryPath::IndirectCount,
    DOLLY_END,
    "indirect-count-dolly-end"
);
golden_test!(
    the_indirect_per_batch_path_matches_its_golden_at_the_dolly_start,
    GeometryPath::IndirectPerBatch,
    DOLLY_START,
    "indirect-per-batch-dolly-start"
);
golden_test!(
    the_indirect_per_batch_path_matches_its_golden_at_the_dolly_end,
    GeometryPath::IndirectPerBatch,
    DOLLY_END,
    "indirect-per-batch-dolly-end"
);
