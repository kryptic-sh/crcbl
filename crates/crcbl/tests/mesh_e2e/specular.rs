//! **A material row's dielectric specular reaches the shading** — glTF's
//! `KHR_materials_specular` and `KHR_materials_ior`, measured on a device.
//!
//! # Why this file exists
//!
//! `GpuMaterial::specular_f0` and `GpuMaterial::specular_f90` replaced a
//! constant: `mesh.slang` shaded every dielectric with the `F0` now named
//! `GpuMaterial::DIELECTRIC_F0` and Schlick's `F90` of one, and the host tests
//! pin the row's bytes and the importer's arithmetic without ever asking
//! whether the lobe reads them. A
//! shader that kept the constant would draw every golden in the tree and pass
//! every host test. So each claim below is a pair of rows that differ in the
//! specular columns alone, drawn under one light, and a reading that tells them
//! apart.
//!
//! Four claims, and they fail for different reasons.
//!
//! * **The row's `F0` and `F90` are the lobe's.** A black dielectric under a
//!   sun is its specular highlight and nothing else; a row with both columns at
//!   zero — glTF's `specularFactor: 0` — reads exactly black where the default
//!   row does not, and a row with `F0` zero and `F90` one still reflects at
//!   grazing incidence.
//! * **The colour is per channel.** A row whose `F0` falls by half from red to
//!   green to blue reads a highlight that falls the same way.
//! * **What the specular layer does not reflect, the diffuse one scatters.** An
//!   ambient-lit dielectric with the layer turned off reads brighter than the
//!   default by exactly the default layer's share, `1 / (1 - DIELECTRIC_F0)`,
//!   and one whose layer reflects everything scatters nothing.
//! * **A rectangle light's split sum scales its bias by the row's `F90`.** The
//!   area-light path never calls the sun's lobe: it weights a polygon integral
//!   by `f0 * dfg.x + f90 * dfg.y`, so with `F0` zero its highlight is the
//!   `F90` term alone, and it reads zero, half and full as `F90` does.
//!
//! # What the frame is
//!
//! [`vertex_v2`](crate::vertex_v2)'s flat quad under the orthographic camera,
//! read at the texel over its centre, where the eye is along the normal. Every
//! effect that could add a term is forced off, for
//! [`mro_page`](crate::mro_page)'s reasons, and no page is named: the rows
//! differ in their factors and in nothing else.

use crate::harness::Headless;
use crate::hdr::HdrTarget;
use crate::mesh_scene::render_mesh_lit;
use crate::vertex_v2::{flat_frame, pixel_at, quad_camera, quad_mesh};
use crcbl::math::{Mat4, Vec3};
use crcbl::render::scene::{Capacities, PageDesc, ProbeGrid, SceneDesc};
use crcbl::render::{
    Antialiasing, DirectionalLight, EffectOverride, EffectRequest, ForwardRenderer, InstanceDesc,
    Light, RectLight, RenderEffects, TransientPool,
};
use crcbl_shaders::mesh::{GpuMaterial, GpuMesh};

/// The UVs the quad carries. Unread — no row names a page — and given for
/// [`quad_mesh`]'s signature.
const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// glTF's default dielectric: every specular and IOR default, which is the
/// lobe this engine drew before the row carried one.
const DEFAULT_ROW: usize = 0;

/// `specularFactor: 0`: no reflectance at either end.
const OFF_ROW: usize = 1;

/// No reflectance at normal incidence and Schlick's full rise towards grazing,
/// so what it reflects is the `F90` end alone.
const GRAZING_ROW: usize = 2;

/// An `F0` that halves from channel to channel, at the default `F90`.
const TINTED_ROW: usize = 3;

/// A layer reflecting everything at normal incidence — the clamp's ceiling,
/// which an IOR of zero reaches.
const FULL_ROW: usize = 4;

/// [`GRAZING_ROW`] with its `F90` halved, so what it reflects is half of what
/// that row's `F90` term carries.
const HALF_F90_ROW: usize = 5;

/// [`HALF_F90_ROW`]'s `F90`.
const HALF_F90: f32 = 0.5;

/// [`TINTED_ROW`]'s `F0`: the default reflectance in red and then halved twice,
/// so a highlight that took one channel for all three reads flat.
const TINTED_F0: [f32; 3] = [0.04, 0.02, 0.01];

/// The rows, in the order the `*_ROW` constants name, each over `base_color`.
fn rows(base_color: [f32; 4]) -> Vec<GpuMaterial> {
    let row = |specular_f0: [f32; 3], specular_f90: f32| GpuMaterial {
        base_color,
        specular_f0,
        specular_f90,
        ..GpuMaterial::UNTINTED
    };
    vec![
        row(GpuMaterial::UNTINTED.specular_f0, 1.0),
        row([0.0; 3], 0.0),
        row([0.0; 3], 1.0),
        row(TINTED_F0, 1.0),
        row([1.0; 3], 1.0),
        row([0.0; 3], HALF_F90),
    ]
}

/// The description every frame here draws: one quad and [`rows`].
fn scene(base_color: [f32; 4]) -> SceneDesc<'static> {
    let rows = rows(base_color);
    assert_eq!(
        rows[DEFAULT_ROW],
        GpuMaterial {
            base_color,
            ..GpuMaterial::UNTINTED
        },
        "the default row must be the untinted row but for its colour, or its readings are \
         not the pictures the goldens hold"
    );
    SceneDesc {
        meshes: vec![quad_mesh(
            "specular quad",
            flat_frame(),
            UVS,
            GpuMesh::MESH_AUTHORED_TANGENTS,
        )],
        materials: rows,
        page: PageDesc::empty(),
        probes: ProbeGrid::default(),
        capacities: Capacities::default(),
    }
}

/// Black, so the diffuse term is exactly zero and a sunlit reading is the
/// specular highlight alone.
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// A white base colour, so an ambient reading is the diffuse weight times the
/// ambient and nothing else.
const WHITE: [f32; 4] = [1.0; 4];

/// The flat ambient [`ambient_only`] lights with. Well under one, so the
/// half-float target holds it with its finest steps.
const AMBIENT: f32 = 0.5;

/// The sun straight along the quad's normal, where the eye is: the highlight's
/// peak, at `V·H` of one.
fn head_on() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::splat(1.0),
        ambient: Vec3::ZERO,
    }
}

/// The sun eighty degrees off the normal, so the half vector sits forty degrees
/// from the eye and Schlick's rise towards `F90` is a measurable share of the
/// reflectance rather than a fifth power of nearly nothing.
fn grazing() -> DirectionalLight {
    let angle = 80.0f32.to_radians();
    DirectionalLight {
        direction: Vec3::new(angle.sin(), 0.0, angle.cos()),
        color: Vec3::splat(1.0),
        ambient: Vec3::ZERO,
    }
}

/// No sun, and a flat ambient: the reading is `diffuse_albedo * AMBIENT`,
/// the one product the diffuse weight scales.
fn ambient_only() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::ZERO,
        ambient: Vec3::splat(AMBIENT),
    }
}

/// The frame texel over the quad's centre, where the eye is along the normal.
fn centre() -> (u32, u32) {
    pixel_at(0.0, 0.0)
}

/// The centre texel of one frame of `description` with the quad drawn under
/// material row `material`, from the raw `Rgba16Float` target.
fn read(description: &SceneDesc<'_>, material: usize, light: &DirectionalLight) -> [f32; 4] {
    read_with(description, material, light, &[])
}

/// [`read`], with `lights` set beside the sun.
fn read_with(
    description: &SceneDesc<'_>,
    material: usize,
    light: &DirectionalLight,
    lights: &[Light],
) -> [f32; 4] {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer = ForwardRenderer::with_scene(
        headless.device.as_ref(),
        headless.queue,
        headless.format,
        description,
    )
    .expect("the forward renderer builds this description");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(Antialiasing::SLOT, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::SHADOWS, Some(false))
            .force(RenderEffects::AMBIENT_OCCLUSION, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_lights(lights);
    renderer
        .add_instance(&InstanceDesc {
            mesh: 0,
            material,
            transform: Mat4::IDENTITY,
        })
        .expect("an instance pool of thousands has room for one quad");
    let mut hdr = Vec::new();
    let _ = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &quad_camera(),
        light,
        Some(&mut hdr),
    );
    let device = headless.device.as_ref();
    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    let (x, y) = centre();
    HdrTarget(hdr).pixel(x, y)
}

/// **A row with no specular reflectance reflects nothing, and one with
/// reflectance only at grazing reflects there** — the row's `F0` and `F90` are
/// the lobe's two ends.
///
/// A black dielectric, so every reading is the highlight. Head on, the default
/// row reads a highlight and the row with both columns zero reads exactly black:
/// a lobe still starting from the old constant would light both alike. At
/// grazing, a row whose `F0` is zero and `F90` one reads above black where the
/// row with both zero does not — so the second column reaches Schlick's curve
/// and is not a constant one in the shader.
///
/// # Sweep
///
/// Measured 2026-09-30 on an RX 7900 XTX under AMD's proprietary Vulkan driver
/// (25.10.36): head on, the default row reads `0.16052246` in every channel and
/// the row with no specular exactly zero; at grazing, `F90` zero reads exactly
/// zero and `F90` one `4.017353e-5` — small, because Schlick's fifth power of
/// `1 - cos 40°` is, and above zero, which is the claim.
///
/// # Sabotage
///
/// `fragmentMain`'s `f0` interpolating from `float3(DIELECTRIC_F0, …)` again in
/// place of `specular_f0_of(material)`, artifacts regenerated: red on that
/// device on 2026-09-30 with `"a row with zero F0 and F90 read [0.16052246,
/// 0.16052246, 0.16052246, 1.0] where the default read [0.16052246, …]"`.
/// `ggx_lobe`'s Fresnel rising to a literal one in place of `f90`: the head-on
/// half stayed green — the fifth-power tail is nothing there — and the grazing
/// half went red with `"a row with zero F0 and F90 reflected at grazing:
/// [4.017353e-5, 4.017353e-5, 4.017353e-5, 1.0]"`, which is why the grazing
/// half exists.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_row_without_specular_reflectance_reflects_nothing() {
    let description = scene(BLACK);
    let default = read(&description, DEFAULT_ROW, &head_on());
    let off = read(&description, OFF_ROW, &head_on());
    eprintln!(
        "{}: head on, the default row reads {default:?} and the row with no specular reads \
         {off:?} at {:?}",
        crate::SUITE,
        centre()
    );
    assert!(
        default[..3].iter().all(|channel| *channel > 0.0),
        "the default black dielectric read {default:?} under a head-on sun; it has a \
         highlight, and a black reading makes the comparison below vacuous"
    );
    assert_eq!(
        off[..3],
        [0.0; 3],
        "a row with zero F0 and F90 read {off:?} where the default read {default:?}; a lobe \
         that ignored the row's specular columns lights both alike"
    );

    let grazing_off = read(&description, OFF_ROW, &grazing());
    let grazing_f90 = read(&description, GRAZING_ROW, &grazing());
    eprintln!(
        "{}: at grazing, F90 zero reads {grazing_off:?} and F90 one reads {grazing_f90:?}",
        crate::SUITE
    );
    assert_eq!(
        grazing_off[..3],
        [0.0; 3],
        "a row with zero F0 and F90 reflected at grazing: {grazing_off:?}"
    );
    assert!(
        grazing_f90[..3].iter().all(|channel| *channel > 0.0),
        "a row with zero F0 and an F90 of one read {grazing_f90:?} at grazing; Schlick's rise \
         towards F90 is the whole of its reflectance, so the row's F90 never reached the lobe"
    );
}

/// **The highlight is coloured channel for channel by the row's `F0`.**
///
/// [`TINTED_F0`] halves from red to green to blue, and head on Schlick's `F` is
/// `F0` to within its fifth-power tail, so the highlight halves the same way.
/// A lobe reading one channel for all three, or the old grey constant, reads
/// flat.
///
/// The ratio is bounded rather than exact: the multi-scatter compensation is a
/// per-channel factor of `F0` too, and it moves each channel by a different few
/// per cent on a surface of this roughness.
///
/// # Sweep
///
/// Measured 2026-09-30 on the RX 7900 XTX: `[0.16052246, 0.08013916,
/// 0.040008545]`, ratios of `2.003` and `2.003`.
///
/// # Sabotage
///
/// The same `f0` sabotage as
/// [`a_row_without_specular_reflectance_reflects_nothing`]'s: red with
/// `"channels 0 and 1 of the tinted highlight read [0.16052246, 0.16052246,
/// 0.16052246, 1.0], a ratio of 1"`.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_specular_colour_tints_the_highlight() {
    let description = scene(BLACK);
    let tinted = read(&description, TINTED_ROW, &head_on());
    eprintln!(
        "{}: the tinted row reads {tinted:?} against an F0 of {TINTED_F0:?}",
        crate::SUITE
    );
    assert!(
        tinted[2] > 0.0,
        "the tinted row's blue read {}, and a black channel makes the ratios vacuous",
        tinted[2]
    );
    for (channel, pair) in tinted[..3].windows(2).enumerate() {
        let ratio = pair[0] / pair[1];
        assert!(
            (TINT_RATIO_MIN..TINT_RATIO_MAX).contains(&ratio),
            "channels {channel} and {} of the tinted highlight read {tinted:?}, a ratio of \
             {ratio}; its F0 halves from each channel to the next, so the highlight must too",
            channel + 1
        );
    }
}

/// The smallest ratio between neighbouring channels of the tinted highlight.
/// The row's `F0` halves, so the ideal is two.
const TINT_RATIO_MIN: f32 = 1.8;

/// The largest such ratio.
const TINT_RATIO_MAX: f32 = 2.2;

/// **The diffuse lobe keeps what the specular layer does not take.**
///
/// A white dielectric under ambient light alone reads `diffuse_albedo *
/// AMBIENT`. `KHR_materials_specular` weights the diffuse term by one minus the
/// layer's reflectance and this engine's default already carries the default
/// layer's share, so with the layer turned off the reading rises by exactly
/// `1 / (1 - DIELECTRIC_F0)`, and with a layer reflecting everything it falls
/// to black.
///
/// **And the neutral row reads the ambient exactly**, which is the half that
/// keeps every lit golden where it was: see `dielectric_diffuse_weight`'s docs
/// in `shaders/mesh.slang` for why the neutral weight is selected rather than
/// computed.
///
/// # Sweep
///
/// Measured 2026-09-30 on the RX 7900 XTX: the default row reads `0.5`, the
/// row with no specular `0.5205078` — `1.0410` of it, against the
/// specification's `1.0417` — and the fully reflective row `0.0`.
///
/// # Sabotage
///
/// `dielectric_diffuse_weight` returning one, artifacts regenerated: red on
/// that device on 2026-09-30 with `"the row with no specular layer read 0.5
/// against the default's 0.5, a ratio of 1"`. Its neutral-row selection
/// removed, leaving the arithmetic alone: red with `"the default white
/// dielectric read [0.49975586, 0.49975586, 0.49975586, 1.0] under an ambient
/// of 0.5"` — one half-float step under, which is the drift the selection
/// exists to stop.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_layer_that_reflects_less_leaves_more_to_the_diffuse() {
    let description = scene(WHITE);
    let default = read(&description, DEFAULT_ROW, &ambient_only());
    let off = read(&description, OFF_ROW, &ambient_only());
    let full = read(&description, FULL_ROW, &ambient_only());
    eprintln!(
        "{}: under ambient light the default row reads {default:?}, the row with no specular \
         {off:?} and the fully reflective row {full:?}",
        crate::SUITE
    );
    // **The neutral row's weight is exactly one**, so a white surface reads
    // the ambient exactly — which is what the diffuse term was before the
    // weight existed, and what every golden holds.
    assert_eq!(
        default[..3],
        [AMBIENT; 3],
        "the default white dielectric read {default:?} under an ambient of {AMBIENT}; its \
         diffuse weight must be exactly one, or every lit golden in the tree moves"
    );
    let wanted = 1.0 / (1.0 - GpuMaterial::DIELECTRIC_F0);
    for channel in 0..3 {
        assert!(
            default[channel] > 0.0,
            "the default row read {default:?} under ambient light, which makes every ratio \
             here vacuous"
        );
        let ratio = off[channel] / default[channel];
        assert!(
            (ratio - wanted).abs() < WEIGHT_TOLERANCE,
            "channel {channel}: the row with no specular layer read {} against the default's \
             {}, a ratio of {ratio}; the diffuse lobe gains the default layer's share, which \
             is {wanted}",
            off[channel],
            default[channel]
        );
        assert!(
            full[channel] < WEIGHT_TOLERANCE * default[channel],
            "channel {channel}: a layer reflecting everything read {} where its diffuse lobe \
             has nothing left to scatter",
            full[channel]
        );
    }
}

/// How far the measured weight may sit from the specification's.
///
/// Two half-float readings of about [`AMBIENT`] each carry a relative error of
/// about one part in two thousand, so their ratio carries about one in a
/// thousand; this is a few times that and far under the four per cent the
/// claim is about.
const WEIGHT_TOLERANCE: f32 = 0.004;

/// No sun and no ambient: whatever a frame under this reads, the lights set
/// beside it put there.
fn dark() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::Z,
        color: Vec3::ZERO,
        ambient: Vec3::ZERO,
    }
}

/// How far in front of the quad the panel hangs.
const PANEL_DISTANCE: f32 = 1.0;

/// Half the panel's edge: wider than the quad, so the mirror direction at the
/// centre texel is well inside it and the polygon integral is a large share
/// of the lobe.
const PANEL_HALF: f32 = 0.6;

/// The radiance leaving the panel's face. Far above one, because the `F90`
/// term is the split sum's bias and head on that is Schlick's fifth power of
/// nearly nothing: the panel has to be bright for its share to land far above
/// the half-float target's smallest steps.
const PANEL_RADIANCE: f32 = 64.0;

/// How far the panel's influence reaches from its centre: far past its own
/// half-diagonal and the quad's corners, so the window is not what shapes the
/// reading.
const PANEL_REACH: f32 = 8.0;

/// A square panel straight in front of the quad and facing it, so the eye's
/// mirror direction at the centre texel points into it.
fn panel() -> Light {
    Light::Rect(RectLight {
        position: Vec3::new(0.0, 0.0, PANEL_DISTANCE),
        radius: PANEL_REACH,
        color: Vec3::splat(PANEL_RADIANCE),
        direction: Vec3::NEG_Z,
        tangent: Vec3::X,
        half_width: PANEL_HALF,
        half_height: PANEL_HALF,
        fill: false,
    })
}

/// **A rectangle light's highlight scales with the row's `F90`** — the
/// area-light path's `f90 * dfg.y`, which the three claims above never reach:
/// each is drawn under the sun, and the sun is shaded by `ggx_lobe`.
///
/// A black dielectric with `F0` zero under [`panel`] alone. The diffuse term is
/// zero, the sun and the ambient are off, and `specular_compensation` is one
/// where `F0` is zero, so the reading is the polygon integral times
/// `f90 * dfg.y` times the panel's radiance and nothing else. The `F90`-one
/// row reads black without the panel and above [`PANEL_FLOOR`] with it, which
/// is what says the rectangle and nothing else lights this texel. Then three
/// rows that differ in `F90` alone: `F90` zero reads exactly black and `F90` a
/// half reads half of `F90` one. A shading path that left `F90` out of the
/// split sum — a literal one where `f90` stands — reads all three alike.
///
/// # Sweep
///
/// Measured 2026-11-04 on the RX 7900 XTX under D3D12 and Vulkan, and on WARP:
/// identical readings on all three. Without the panel the `F90`-one row reads
/// exactly zero; under it, `F90` one reads `0.0049324036`, `F90` a half
/// `0.0024662018` — a ratio of exactly `0.5` — and `F90` zero exactly zero.
///
/// # Sabotage
///
/// The shaders are committed artifacts, so the sabotage was on the host:
/// `GpuMaterial::to_bytes` writing a literal one in place of `specular_f90`.
/// Red on D3D12 on 2026-11-04 with `"a row with zero F0 and F90 read
/// [0.0049324036, …] under the panel where an F90 of one read [0.0049324036,
/// …]"`. It writing one wherever `specular_f90` is above zero, and zero
/// elsewhere: red with `"channel 0: F90 0.5 read 0.0049324036 against F90 one's
/// 0.0049324036, a ratio of 1"`, which is why the half row exists. Both reach
/// every light, not the rectangle's arm alone — the sun's grazing half above
/// went red under the first — so they show this test can fail, and the panel
/// being the only light is what makes its readings the area path's.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_rectangle_lights_the_f90_term_by_the_row() {
    let description = scene(BLACK);
    let unlit = read_with(&description, GRAZING_ROW, &dark(), &[]);
    let full = read_with(&description, GRAZING_ROW, &dark(), &[panel()]);
    let half = read_with(&description, HALF_F90_ROW, &dark(), &[panel()]);
    let off = read_with(&description, OFF_ROW, &dark(), &[panel()]);
    eprintln!(
        "{}: without the panel F90 one reads {unlit:?}; under it, F90 one reads {full:?}, F90 \
         {HALF_F90} reads {half:?} and F90 zero reads {off:?} at {:?}",
        crate::SUITE,
        centre()
    );
    assert_eq!(
        unlit[..3],
        [0.0; 3],
        "the F90-one row read {unlit:?} with no light on; something other than the panel lights \
         this frame, so the readings below are not the area light's"
    );
    assert!(
        full[..3].iter().all(|channel| *channel > PANEL_FLOOR),
        "a row with zero F0 and an F90 of one read {full:?} under the panel alone, under the \
         {PANEL_FLOOR} floor; the rectangle is not lighting this texel, and every comparison \
         below would be vacuous"
    );
    assert_eq!(
        off[..3],
        [0.0; 3],
        "a row with zero F0 and F90 read {off:?} under the panel where an F90 of one read \
         {full:?}; the area light's split sum is not scaling its bias by the row's F90"
    );
    for channel in 0..3 {
        let ratio = half[channel] / full[channel];
        assert!(
            (ratio - HALF_F90).abs() < F90_RATIO_TOLERANCE,
            "channel {channel}: F90 {HALF_F90} read {} against F90 one's {}, a ratio of \
             {ratio}; with F0 zero the highlight is linear in F90",
            half[channel],
            full[channel]
        );
    }
}

/// The least the `F90`-one row may read under [`panel`], in every channel.
///
/// A floor against a texel the rectangle does not reach, not a prediction:
/// far over the half-float target's steps near zero and far under the reading.
const PANEL_FLOOR: f32 = 0.001;

/// How far the half row's share may sit from [`HALF_F90`]. Two half-float
/// readings carry about one part in a thousand between them; this is a few
/// times that and far under the half the claim is about.
const F90_RATIO_TOLERANCE: f32 = 0.01;
