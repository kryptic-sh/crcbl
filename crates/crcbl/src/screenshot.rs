//! Offscreen render → readback → golden image — the `crcbl screenshot` path.
//!
//! Opens a GPU backend, creates an offscreen surface and swapchain, renders
//! one frame of the [`Scene`] the caller named, and reads the pixels back.
//!
//! # Why there is more than one scene
//!
//! `docs/plan/02-vulkan-backend.md`'s shader-portability rule 5: a shader can
//! compile cleanly to SPIR-V, WGSL, MSL and DXIL and *mean something different*
//! on each, and no lint can see it — `SV_InstanceID` lowers to
//! `InstanceIndex - BaseInstance` on SPIR-V and to a bare
//! `@builtin(instance_index)` on WGSL, which is why every sprite batch after the
//! first drew the first batch's instances while every gate stayed green. The
//! only detector is rendering the same scene through two backends and comparing
//! pixels, which is what `tests/run-cross-backend-e2e.sh` does — and it can only
//! catch what it draws. [`Scene::Cube`] exercises `mesh.slang` and
//! `tonemap.slang`; [`Scene::Sprite`] and [`Scene::Ui`] are here so
//! `sprite.slang` and `ui.slang` — the two with an actual history of divergence
//! — are covered too.
//!
//! `SV_VertexID` turned out to be the same story, and it is why [`Scene::Cube`]
//! draws the mesh pool's *second* resident beside the cube: a base vertex only
//! means something for a mesh that is not at base 0, and this is the only frame
//! in the tree that renders one through both backends.
//!
//! What comes back is the swapchain image's bytes *as they are in memory*, in
//! [`OffscreenSetup::format`]'s channel order — which on an ordinary desktop
//! surface is BGRA, not RGBA. Turning them into a `crcbl_golden::Image` is
//! the caller's job and needs `Image::from_readback` with the matching
//! `ChannelOrder`, not `Image::from_rgba8`; this module deliberately does not
//! guess, because the format is the thing it knows and the image type is not
//! its dependency to reach for.
//!
//! This module is the render half of the CLI subcommand; the CLI module
//! owns the argument parsing and I/O.
//!
//! # Native only
//!
//! The whole path blocks: [`crate::backend::open`], a blocking device creation,
//! and a `std::thread::sleep` poll loop waiting for the readback copy to land.
//! The browser's main thread may not block, so this module is
//! `#[cfg(not(target_arch = "wasm32"))]` in `lib.rs` and a wasm build that
//! reaches for it fails to *compile* rather than hanging on the first
//! screenshot — the rule [`crate::backend`] states and
//! [`Instance::create_device`] established.
//!
//! Nothing here is wanted in a browser at P5: it is a command-line tool's back
//! end, not something a game calls per frame. If it ever is, the polled shapes
//! it would have to be rewritten onto already exist —
//! [`Instance::request_device`] and
//! [`Device::poll_readback`].
//!
//! # Which adapter drew it
//!
//! This module used to pick `adapters().first()` and never report which one
//! that was. On `windows-latest` that adapter is not a usable device, and the
//! frame died on its first buffer with `DXGI_ERROR_DEVICE_REMOVED` before
//! anything was drawn — see [`crate::adapter`], which is now what chooses.
//! [`crate::adapter::ADAPTER_ENV_VAR`] names a device class, a miss is a hard
//! failure rather than a fallback, and [`OffscreenSetup::adapter`] reports what
//! answered so a harness can check the pin landed.
//!
//! **The remaining half is the CLI's.** `crcbl screenshot` installs no logger,
//! so the backends' own adapter lines still go nowhere and the subcommand prints
//! nothing about the device; closing that means a `--json` field naming the
//! adapter, which is `crcbl-cli`'s call to make. `tests/run-render-e2e.sh` reads
//! [`OffscreenSetup::adapter`] out of its suite instead, the way `run-vk-e2e.sh`
//! reads the adapter line out of its own.

use std::time::Duration;

use crate::hal::{
    Barriers, BufferDesc, BufferImageCopy, BufferUsage, CommandEncoderDesc, Device, DeviceDesc,
    Extent3d, Features, Format, ImageAspect, ImageBarrier, ImageSubresourceLayers,
    ImageSubresourceRange, Instance, MemoryLocation, Offset3d, PresentInfo, PresentMode,
    QueueHandle, QueueKind, ReadbackDesc, ReadbackState, ResourceState, SubmitInfo, SurfaceError,
    SurfaceHandle, SurfaceTarget, SwapchainDesc, SwapchainHandle,
};
use crate::render::{
    Camera, DirectionalLight, FontAtlas, ForwardRenderer, FrameCounters, GraphError, Projection,
    RenderGraph, SampleMode, SheetDesc, SheetId, Sprite, SpriteRenderer, TransientPool, UiRenderer,
};
use crate::ui::draw_list::DrawList;

// ---------------------------------------------------------------------------
// Scenes
// ---------------------------------------------------------------------------

/// What a screenshot draws.
///
/// One variant per engine shader pair that has pixels of its own, because the
/// cross-backend comparison this feeds can only catch divergence in a shader it
/// actually ran — see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The sandbox's lit cube through [`ForwardRenderer`]: `mesh.slang` into an
    /// HDR target and `tonemap.slang` back out of it.
    ///
    /// The default, because it is the frame every caller of this module drew
    /// before there was anything to choose between.
    ///
    /// The pyramid beside it is the mesh pool's second resident, and it is here
    /// for the reason the module docs give: it is the only geometry in the tree
    /// at a non-zero base vertex, so it is the only thing this comparison can
    /// use to tell the four targets' `SV_VertexID` lowerings apart.
    ///
    /// **There are three of it**, and they are the same mesh at the same
    /// orientation differing in nothing but their material id — which is what
    /// makes this frame the observable for
    /// `docs/plan/03-gpu-driven-rendering.md` §3.2's material table. The three
    /// are one row and two single-column edits of it: see `TINTED_PYRAMID_AT`,
    /// whose row differs in its base-colour *factor*, and
    /// `TEXTURED_PYRAMID_AT`, whose row differs in its base-colour *texture*.
    /// One pair per column, so neither column's evidence is the other's.
    #[default]
    Cube,
    /// `docs/plan/25-lod.md`'s dunes patch through [`ForwardRenderer`], seen
    /// from its own near edge.
    ///
    /// **The scene that proves a device with no amplification stage can draw a
    /// cluster DAG at all.** [`Scene::Cube`]'s three residents are flat meshes,
    /// so a renderer whose level selection picked nothing — or picked a level it
    /// then failed to draw — produces that frame unchanged. This one is the only
    /// geometry in the tree with a hierarchy, so on the two indirect tails every
    /// triangle of it arrives through a level `draw_gen.slang` chose.
    ///
    /// The camera is `DUNES_CAMERA_AT`: at the patch's near edge, low, looking
    /// along it as it recedes — which is where the near end and the far end are
    /// tens of times apart in distance and the metric has something to say.
    Dunes,
    /// Four sprites over three [`SpriteRenderer`] batches: `sprite.slang`.
    Sprite,
    /// `docs/plan/18-render-features.md`'s light list: the cube scene's geometry
    /// under three coloured **point** lights and a sun turned down far enough
    /// that they are what is lighting it.
    ///
    /// The scene the clustered-forward slice exists for, and it is deliberately
    /// the same geometry as [`Scene::Cube`]: the two goldens differ in the light
    /// list and in nothing else, so a diff between them is the feature rather
    /// than a new arrangement of objects.
    Lights,
    /// `docs/plan/18-render-features.md`'s **spot** light: one cone of light on
    /// a flat floor, seen from straight above.
    ///
    /// **The only frame in the tree that draws a cone.** [`Scene::Lights`] is
    /// three point lights, so `mesh.slang`'s `spot_cone` compiled to all four
    /// targets and lit no pixel anywhere — and a cone test with its sign
    /// inverted, its two angles the wrong way round, or a step where its ramp
    /// should be draws a plausible picture in each case and passes every other
    /// golden here unchanged.
    ///
    /// The geometry is the cube alone, scaled into a floor, because what this
    /// scene needs is a surface for the cone to land on and nothing else in the
    /// frame to read the shape off instead. See this module's `spot_camera` for
    /// why the camera looks straight down and `SPOT_HEIGHT` for what the light's
    /// height buys the pixel assertions.
    Spot,
    /// `docs/plan/18-render-features.md`'s **shadowed spot**: [`Scene::Spot`]'s
    /// floor and cone with an object standing in the light.
    ///
    /// **The only frame in the tree where a light other than the sun occludes.**
    /// A shadow map that is never written, never sampled, or sampled through a
    /// matrix that disagrees with the one it was rendered with all produce a
    /// plainly plausible picture here — an evenly lit pool — so the assertions
    /// in `tests/render_e2e.rs` are about *where* the dark region is, and
    /// `crcbl-vk`'s spot suite is what moves the caster and watches it follow.
    ///
    /// It is a scene of its own rather than an occluder added to [`Scene::Spot`]
    /// for two reasons, and the first is the stronger: that scene's assertions
    /// are a radial profile out from the frame's centre through the cone's
    /// penumbra, and an object in the middle of it destroys exactly the
    /// measurement it exists to make. The second is that its golden would have
    /// had to be re-blessed for a feature it is not evidence about.
    ///
    /// The camera looks straight down and the light comes in at 45°, which is
    /// the arrangement that separates the shadow from the caster: with both
    /// overhead the shadow hides under the object that casts it. See
    /// `spot_shadow_camera` and `SPOT_SHADOW_LIGHT_AT`.
    SpotShadow,
    /// `docs/plan/18-render-features.md`'s **shadowed point light**: one light
    /// low over a floor with a caster on either side of it, seen from above.
    ///
    /// **The only frame in the tree where one light occludes in more than one
    /// direction**, and that is the whole of what it is for. A point light's map
    /// is six atlas tiles and the fragment stage picks one of them from the
    /// direction to the light, so five of the six can be wrong — sampled from the
    /// neighbouring face, or from a fixed one — and the frame still has a shadow
    /// in it. The two casters are on *different* faces, so a frame that got the
    /// face selection wrong loses one of the two shadows while keeping the other,
    /// which `tests/render_e2e.rs` asserts side by side rather than one at a
    /// time.
    ///
    /// See this module's `POINT_LIGHT_UP` for why the light is low — a light
    /// high over the floor puts every receiver on the `-Y` face and the scene
    /// stops distinguishing anything — and `POINT_CASTER_AT` for where the
    /// casters stand.
    PointShadow,
    /// `docs/plan/18-render-features.md`'s **screen-space ambient occlusion**:
    /// the inside of a box, looked straight down into, lit almost entirely by
    /// ambient.
    ///
    /// **The only frame in the tree whose subject is a term that darkens nothing
    /// else.** AO multiplies `frame.ambient.rgb` alone, so a scene with a strong
    /// key light shows it as a rounding error: this one turns the sun down to a
    /// trace and leaves the ambient at full, which makes the floor's brightness
    /// very nearly the occlusion value itself.
    ///
    /// It is a scene of its own rather than an occluder added to
    /// [`Scene::Spot`], for [`Scene::SpotShadow`]'s two reasons and a third that
    /// is stronger than either: **the sun points straight down**. A vertical
    /// light casts no shadow from a vertical wall, so the two floor bands
    /// `tests/render_e2e.rs` compares receive identical direct light, identical
    /// ambient and identical Lambert — and the *only* thing left that can
    /// separate them is the occlusion term. Under any other light the
    /// measurement would be a shadow-map result wearing AO's name.
    ///
    /// The geometry is the open box alone — see
    /// `crcbl_shaders::mesh::OPEN_BOX_FACES`, whose five faces point inward —
    /// scaled by `ao_box` into a long narrow **trough**, so that its two long
    /// walls are in frame and its two ends are not. See `ao_camera` for why the
    /// view is straight down and `AO_RUN` for why the trough is not a square
    /// room.
    Ao,
    /// Rectangles, an outline and glyph-atlas text through [`UiRenderer`]:
    /// `ui.slang`.
    Ui,
}

/// How far from the cube's own column each pyramid sits, in world units.
///
/// The camera is two units back with a 60° vertical field of view, so at
/// `z = 0` the frame is about 2.3 units tall and 3.1 wide at 4:3. The cube
/// spans `±0.5` and each pyramid `±0.4`, so a column at `±1.05` sits fully
/// inside the frame with a gap on both sides of it — and a gap is what makes
/// "the pyramid drew the cube's vertices" a visibly different picture rather
/// than an overlap.
const PYRAMID_COLUMN: f32 = 1.05;

/// How far above or below the cube's own row each pyramid sits.
///
/// The pyramids used to share the cube's row, and a **third** of them is what
/// moved them off it: two rows are what a scene needs to hold two pairs, and
/// there is no room for a third column — the cube is 0.5 wide and each pyramid
/// 0.4, so a fourth object beside them would leave the frame.
///
/// `0.55` is what fits. A pyramid spans `-0.4 ..= 0.5` about its own origin and
/// the frame is `±1.15` tall, so a row at `+0.55` reaches `1.05` and one at
/// `-0.55` reaches `-0.95`: both inside, with `0.2` of clear sky between the
/// rows so no two pyramids touch.
const PYRAMID_ROW: f32 = 0.55;

/// Where [`Scene::Cube`] puts the plain pyramid: top left.
///
/// It is the one both other pyramids are compared against, and its material is
/// the untinted, untextured row — so each of the two below differs from *this*
/// object in exactly one material column.
const PYRAMID_AT: glam::Vec3 = glam::Vec3::new(-PYRAMID_COLUMN, PYRAMID_ROW, 0.0);

/// Where [`Scene::Cube`] puts the **factor** pyramid: top right, beside
/// [`PYRAMID_AT`].
///
/// The two are the same mesh at the same orientation and the same size, and the
/// only field their instances differ in is the material id — which is the whole
/// of what makes this frame evidence about §3.2's material table. A frame in
/// which the two pyramids are the same colour is a frame where that id indexed
/// nothing, and it is a *visibly* different frame rather than a subtly wrong
/// one.
///
/// Beside it rather than anywhere else so the difference is read across a row:
/// the two are at the same height, lit by the same directional light, and the
/// only thing that can make them different colours is the factor in the row
/// their id names.
const TINTED_PYRAMID_AT: glam::Vec3 = glam::Vec3::new(PYRAMID_COLUMN, PYRAMID_ROW, 0.0);

/// Where [`Scene::Cube`] puts the **texture** pyramid: below [`PYRAMID_AT`].
///
/// [`TINTED_PYRAMID_AT`]'s argument moved one column of the material row along.
/// This instance's material has [`PYRAMID_AT`]'s factor exactly and a different
/// base-colour page layer, so the pair above and below each other is the
/// observable for §3.2's *texture indices* where the pair across the top row is
/// the observable for its *factors*.
///
/// The layer it names is four unequal texels rather than a flat colour, so this
/// pyramid's faces are quartered where the plain one's are flat — which means
/// the frame also fails if the texture coordinate never reached the fragment
/// stage, not only if the index did.
///
/// Below rather than beside, because there is no third column: see
/// [`PYRAMID_ROW`]. The bottom-right corner is left empty for the same reason a
/// fourth pyramid would prove nothing new — a row differing in *both* columns
/// is a picture neither pair could be told from.
const TEXTURED_PYRAMID_AT: glam::Vec3 = glam::Vec3::new(-PYRAMID_COLUMN, -PYRAMID_ROW, 0.0);

/// How far in front of the geometry [`Scene::Lights`] hangs its point lights.
///
/// The cube and the pyramids sit at `z = 0` and the camera is on `+Z`, so a
/// light nearer than they are lights the faces the camera can see. Near enough
/// that the falloff is steep across one object, which is what makes the three
/// pools of colour separate rather than a wash.
const LIGHT_Z: f32 = 1.1;

/// How far apart two of these lights are, at their closest.
///
/// The two left-hand pyramids, which [`PYRAMID_ROW`] puts one above and one
/// below the cube's row. [`LIGHT_RADIUS`] is held under this, which is what
/// makes each pool one colour rather than three overlapping ones.
const LIGHT_SPACING: f32 = 2.0 * PYRAMID_ROW;

/// How far a [`Scene::Lights`] point light reaches, in world units.
///
/// **A hard bound**: `crcbl_render::PointLight::radius` is the radius the
/// shading window reaches zero at and the radius the clustering pass culls
/// against, so this is also how many froxels each light lands in.
///
/// Under [`LIGHT_SPACING`] and over the light's own distance to the faces below
/// it, so each pyramid is lit by its own light and by no other — which is what
/// makes the golden legible: a fragment stage that ignored the froxel grid and
/// summed the whole list would put all three colours on all three pyramids, and
/// that is a picture, not an error.
const LIGHT_RADIUS: f32 = 1.05;

/// How bright each [`Scene::Lights`] point light is, before its colour.
///
/// Above 1.0, like the sun's: the scene target is `Rgba16Float` and the tonemap
/// pass is what brings it back, so a light that peaked at 1.0 would be one this
/// pipeline could not tell from a darker one. Below the point where the
/// specular lobe clips, so the pools carry a gradient rather than a flat white
/// core.
const LIGHT_INTENSITY: f32 = 2.0;

/// The three [`Scene::Lights`] point lights: one over each pyramid, in a colour
/// family none of the materials is.
///
/// Red, blue and green rather than three whites, so which light lit a pixel is
/// legible in the golden — a wrong froxel assignment shows up as a pool in the
/// wrong place rather than as a slightly different brightness.
fn scene_lights() -> [crcbl_render::Light; 3] {
    const {
        assert!(
            LIGHT_RADIUS < LIGHT_SPACING,
            "a radius reaching the next pyramid puts two colours in one pool, and the \
             golden stops saying which light lit what"
        );
    }
    let at = |x: f32, y: f32, colour: glam::Vec3| {
        crcbl_render::Light::Point(crcbl_render::PointLight {
            position: glam::Vec3::new(x, y, LIGHT_Z),
            radius: LIGHT_RADIUS,
            color: colour * LIGHT_INTENSITY,
        })
    };
    // **Each light's colour is chosen against the material under it**, because a
    // fragment's colour is the product of the two: [`PYRAMID_TINT`] makes the
    // top-right pyramid strongly blue, so a green light there would come out
    // blue and say nothing about which light lit it. The blue light goes where
    // the blue material is; the two neutral pyramids take the other two.
    [
        at(
            -PYRAMID_COLUMN,
            PYRAMID_ROW,
            glam::Vec3::new(1.0, 0.15, 0.1),
        ),
        at(PYRAMID_COLUMN, PYRAMID_ROW, glam::Vec3::new(0.15, 0.3, 1.0)),
        at(
            -PYRAMID_COLUMN,
            -PYRAMID_ROW,
            glam::Vec3::new(0.1, 1.0, 0.2),
        ),
    ]
}

/// [`DirectionalLight::default`] at `key` of its brightness and `ambient` of its
/// ambient, for a scene whose subject is the lights beside the sun.
///
/// **Turned down rather than removed**, and that is a claim each of those scenes
/// makes: the sun is a row of the same list the punctual lights are rows of, so
/// a frame in which it stopped contributing would be as wrong as one in which
/// they did.
///
/// Two factors rather than one because the ambient does a different job — it is
/// what keeps a face turned away from every light dark rather than black — and a
/// scene wants to dim the key light much further than the floor under it.
fn dimmed_sun(key: f32, ambient: f32) -> crcbl_render::DirectionalLight {
    let bright = crcbl_render::DirectionalLight::default();
    crcbl_render::DirectionalLight {
        direction: bright.direction,
        color: bright.color * key,
        ambient: bright.ambient * ambient,
    }
}

/// The sun [`Scene::Lights`] runs under: dim enough that the pools of colour are
/// unmistakably the three point lights' work.
fn dim_sun() -> crcbl_render::DirectionalLight {
    dimmed_sun(0.12, 0.35)
}

/// The sun [`Scene::Spot`] runs under, which is dimmer still.
///
/// That scene's claim is a **ratio** — the cone's core against the floor outside
/// it — so a sun bright enough to be a large part of that floor is a sun that
/// decides the ratio. Dim enough that the floor is unmistakably the dark side of
/// the cone's edge, and not so dim that it is black: a black floor would make
/// "dark outside the cone" a statement about an unpainted frame.
///
/// [`SPOT_INTENSITY`]'s doc says what the other end of the ratio is.
fn spot_sun() -> crcbl_render::DirectionalLight {
    dimmed_sun(0.03, 0.09)
}

/// How far above the floor [`Scene::Spot`] hangs its light.
///
/// **Large next to the cone's own footprint, and that ratio is what the pixel
/// assertions rest on.** Distance falloff and Lambert fall off across the pool
/// as well as the cone does, so on a low light a cone that was a hard cut rather
/// than a ramp would still leave a gradient behind it and "the penumbra varies"
/// would be satisfied by arithmetic that has nothing to do with the cone. From
/// up here the far edge of the pool is barely further from the light, and barely
/// more oblique to it, than the axis is — so what changes across the penumbra is
/// the cone and very little else.
const SPOT_HEIGHT: f32 = 1.6;

/// The radius of the cone's bright core where it lands on [`Scene::Spot`]'s
/// floor.
///
/// Written as a radius on the floor rather than as a half-angle in radians,
/// which is also how [`spot_light`] passes it: the radius is what the frame
/// shows and what the assertions talk about, and the angle is the arctangent of
/// it over [`SPOT_HEIGHT`].
const SPOT_CORE_RADIUS: f32 = 0.14;

/// The radius at which the cone has closed, on the same terms.
///
/// The band between this and [`SPOT_CORE_RADIUS`] is the penumbra — the part of
/// the picture that separates a working ramp from a boolean — so the two are set
/// far enough apart that it is tens of pixels wide under the camera
/// [`spot_camera`] puts over it, and `SPOT_PENUMBRA_MIN` in
/// `tests/render_e2e.rs` is what holds that to a number.
const SPOT_EDGE_RADIUS: f32 = 0.4;

/// How far [`Scene::Spot`]'s light reaches, in world units.
///
/// Twice [`SPOT_HEIGHT`], so the quartic window that
/// `crcbl_render::PointLight::radius` documents is nowhere near its zero where
/// the cone lands: the pool's edge is then the cone's doing rather than the
/// radius', which is the thing being drawn.
const SPOT_REACH: f32 = 2.0 * SPOT_HEIGHT;

/// How bright [`Scene::Spot`]'s light is, before its colour.
///
/// Well above [`LIGHT_INTENSITY`] because the falloff is an inverse square and
/// [`SPOT_HEIGHT`] is the distance it runs over — the product is what matters,
/// not either number. Chosen so the axis lands *under* the top of the swapchain
/// once the tonemap has brought it back: a core that clipped would be a plateau
/// with no shape in it, and the cone's own core is part of what this frame is
/// meant to show.
const SPOT_INTENSITY: f32 = 5.0;

/// How far above the floor [`Scene::Spot`]'s camera stands.
///
/// Lower than [`SPOT_HEIGHT`], so the light is behind the eye and nothing in the
/// frame is between the two. Far enough up that the frame's short half-axis on
/// the floor is comfortably wider than [`SPOT_EDGE_RADIUS`] — the pool then sits
/// well inside the frame with dark floor between it and every edge, which is
/// what lets a profile out from the centre run past the cone and reach that
/// floor.
const SPOT_CAMERA_UP: f32 = 1.2;

/// How much [`Scene::Spot`] scales the cube by to get its floor.
///
/// Uniform, so each face keeps the axis-aligned normal it was built with, and
/// large enough that the `+Y` face runs past every edge of the frame — the scene
/// is one flat lit surface and the cone on it, with no silhouette anywhere for a
/// reader or an assertion to take the shape off instead.
const SPOT_FLOOR_SCALE: f32 = 8.0;

/// [`Scene::Spot`]'s floor: the cube scaled by [`SPOT_FLOOR_SCALE`] and dropped
/// so its `+Y` face is the plane `y = 0`.
///
/// The cube spans half a unit either side of its origin, so the drop is half the
/// scale.
fn spot_floor() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, -0.5 * SPOT_FLOOR_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(SPOT_FLOOR_SCALE))
}

/// The camera [`Scene::Spot`] is drawn with: straight down at the floor.
///
/// **Overhead rather than oblique, and the pixel assertions rest on that.** With
/// the cone's axis, the floor's normal and the view direction all `Y`, every
/// term in the shading — the cone, the distance falloff, Lambert and the
/// specular lobe alike — is a function of the distance from the frame's centre
/// alone, and each of them falls off with it. So the pool is a circle about a
/// pixel the test can name, and a profile out from that pixel is the cone's own
/// cross-section rather than a diagonal cut through an ellipse whose position a
/// test would have to reconstruct the projection to find.
///
/// `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the floor's
/// `+Z` axis at the top of the frame.
fn spot_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, SPOT_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// [`Scene::Spot`]'s one light: a cone pointing straight down at the floor.
///
/// Near-white rather than a colour of its own, because the floor's albedo is the
/// cube's green `+Y` face and a coloured light would tint the pool where the
/// point of it is to show the pool's *shape*.
fn spot_light() -> crcbl_render::Light {
    crcbl_render::Light::Spot(crcbl_render::SpotLight {
        position: glam::Vec3::new(0.0, SPOT_HEIGHT, 0.0),
        radius: SPOT_REACH,
        color: glam::Vec3::new(1.0, 0.95, 0.85) * SPOT_INTENSITY,
        // Along the cone, away from the light — the opposite convention from the
        // sun's, which `crcbl_render::SpotLight::direction` is where it is
        // spelled out.
        direction: glam::Vec3::NEG_Y,
        inner_angle: (SPOT_CORE_RADIUS / SPOT_HEIGHT).atan(),
        outer_angle: (SPOT_EDGE_RADIUS / SPOT_HEIGHT).atan(),
    })
}

/// How far apart [`Scene::Ao`]'s two facing walls stand, in world units.
///
/// **This is the scene's whole subject**: a trough, narrow enough that its walls
/// close over the floor between them and wide enough that the middle of that
/// floor is well outside `crcbl_render::ForwardRenderer`'s occlusion radius. The
/// bands `tests/render_e2e.rs` measures sit a tenth of a unit off each wall.
const AO_TROUGH: f32 = 1.6;

/// How far [`Scene::Ao`]'s trough runs, in world units.
///
/// **Several times [`AO_TROUGH`], and that asymmetry is what makes the scene
/// measurable.** The two ends are then far enough out to be off frame *and* many
/// occlusion radii from the middle — so a band out along the run is open floor
/// while a band the same distance out across the trough is against a wall. Two
/// bands at the same distance from the eye, on the same surface, under the same
/// light, differing in occlusion alone: a square room has no such pair, because
/// every point at a corner's distance from the centre is itself in a corner.
const AO_RUN: f32 = 6.0;

/// How tall [`Scene::Ao`]'s walls are, in world units.
///
/// Taller than the trough is wide, so each wall closes a good half of the
/// hemisphere over the floor beside it rather than a sliver of it — occlusion is
/// a solid angle, and a kerb would subtend almost none.
const AO_WALL: f32 = 2.0;

/// How far above the floor [`Scene::Ao`]'s camera stands, in world units.
///
/// Just above the wall tops, looking into the trough. It also sets the scale of
/// the picture: with the 60° vertical field of view `ao_camera` uses, the frame's
/// short half-axis on the floor is `AO_CAMERA_UP * tan(30°)` — so this is what
/// puts both walls inside the frame and both ends of the run outside it, which is
/// the framing every band below depends on.
const AO_CAMERA_UP: f32 = 2.2;

/// [`Scene::Ao`]'s trough: the open box scaled to
/// [`AO_RUN`] × [`AO_WALL`] × [`AO_TROUGH`] and lifted so its floor is the plane
/// `y = 0`.
///
/// **A non-uniform scale, and it is safe on this mesh alone.** Every face of
/// `crcbl_shaders::mesh::OPEN_BOX_FACES` is axis aligned, so an axis-aligned
/// scale leaves each normal on its own axis and `mesh.slang` renormalises what it
/// is handed. A mesh with an oblique face would need the inverse transpose, which
/// nothing in this engine builds.
///
/// **Centred on the camera's axis, and that is not decoration.** A box this large
/// slid sideways from the eye stops drawing altogether — the frame comes back as
/// clear colour, on every geometry path, with the instance passing a host-side
/// frustum test. It reproduces without any of this slice's passes, so it is not
/// theirs; the scene is arranged to stay clear of it rather than to chase it.
fn ao_box() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.5 * AO_WALL, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::new(AO_RUN, AO_WALL, AO_TROUGH))
}

/// Where [`Scene::Ao`] parks the cube.
///
/// [`ForwardRenderer::begin_frame`] always places the cube, and this scene has no
/// use for it: a lit box in the middle of the trough is a second shape in a frame
/// whose whole content is a floor and the two walls closing over it, and it would
/// stand exactly where the unoccluded bands are measured. So it is put out along
/// the run, past the frame and inside the trough — inside, so it cannot poke
/// through a wall on a future change to the framing.
fn ao_parked_cube() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.45 * AO_RUN, 0.5, 0.0))
}

/// The camera [`Scene::Ao`] is drawn with: straight down into the trough.
///
/// **Overhead, and the band placement rests on it.** With the view direction
/// along `Y` and the floor's normal along `Y`, four points the same distance from
/// the frame's centre on that floor are the same distance from the eye, carry the
/// same normal and — the sun being directional — take the same direct light. Two
/// of them are against a wall and two are on open floor, and the occlusion term is
/// then the only thing that can tell the pairs apart.
///
/// `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the trough's
/// `+Z` wall at the top of the frame, exactly as `spot_camera` does.
fn ao_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, AO_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The sun [`Scene::Ao`] runs under: **straight down**, and barely there.
///
/// Both halves are load-bearing and neither is a taste.
///
/// * **Straight down** — a vertical light throws no shadow from a vertical wall,
///   so the floor is lit identically right up to where it meets one. Under the
///   default sun's tilt a wall [`AO_WALL`] tall would lay a shadow across the
///   floor beside it and the bands would be measuring the shadow map.
///   `crcbl_render::shadow` picks a second up vector for exactly this direction,
///   so the cascades are built rather than degenerate.
/// * **Barely there** — occlusion scales the ambient term and nothing else, so a
///   bright key light is something the measurement has to see through. Turned
///   down rather than removed, for `dimmed_sun`'s reason: a sun that stopped
///   contributing is a row of the light list that stopped working, and a scene
///   without one would not notice.
fn ao_sun() -> crcbl_render::DirectionalLight {
    crcbl_render::DirectionalLight {
        direction: glam::Vec3::Y,
        ..dimmed_sun(0.01, 1.0)
    }
}

/// Where [`Scene::SpotShadow`] puts its light.
///
/// **45° from vertical, and that angle is the whole scene.** The camera looks
/// straight down, so a light straight down too would throw every shadow directly
/// under the object that casts it, where the object's own image covers it — and
/// a camera lower than the light magnifies the caster more than the light does,
/// so the shadow can never grow out from under it. Tilting the light moves the
/// shadow sideways by the caster's height, which at 45° is one for one.
///
/// Close enough in that the cone's pool still fits the frame at
/// [`SPOT_SHADOW_OUTER_ANGLE`], and high enough that the tilt is a tilt rather
/// than a grazing light whose pool runs off the bottom of the frame.
const SPOT_SHADOW_LIGHT_AT: glam::Vec3 = glam::Vec3::new(0.0, 1.2, 1.2);

/// How far [`Scene::SpotShadow`]'s light reaches, in world units.
///
/// Twice its distance to the floor's centre, on [`SPOT_REACH`]'s terms exactly:
/// the pool's edge is then the cone's doing rather than the radius'.
const SPOT_SHADOW_REACH: f32 = 3.4;

/// The half-angle at which [`Scene::SpotShadow`]'s cone closes, in radians.
///
/// Written as an angle rather than as a radius on the floor — unlike
/// [`SPOT_EDGE_RADIUS`] — because this cone lands on the floor at 45° and its
/// pool is an ellipse, so there is no one radius to name it by. Wide enough that
/// the pool covers the caster and the whole of the shadow it throws, narrow
/// enough that dark floor is still visible in the frame's corners.
const SPOT_SHADOW_OUTER_ANGLE: f32 = 0.28;

/// The half-angle of its bright core.
///
/// Far enough inside [`SPOT_SHADOW_OUTER_ANGLE`] that the shadow and the
/// penumbra are separate things in the frame: this scene's claim is about a dark
/// region with a lit region beside it, and a cone that was all penumbra would put
/// a gradient across both.
const SPOT_SHADOW_INNER_ANGLE: f32 = 0.18;

/// How much [`Scene::SpotShadow`] scales the pyramid by to get its caster.
///
/// The pyramid rather than a second cube, because the cube is already the floor
/// and one resident cannot be two instances of different sizes here. Small
/// enough that the pool has lit floor on both sides of the shadow, large enough
/// that the shadow is tens of pixels across at the extent
/// `tests/render_e2e.rs` renders.
const SPOT_SHADOW_CASTER_SCALE: f32 = 0.5;

/// How far above the floor [`Scene::SpotShadow`]'s camera stands.
///
/// Low enough that the pool fills a good part of the frame — the assertions
/// measure bands of it, and a distant camera makes each band a handful of
/// pixels. The light is at [`SPOT_SHADOW_LIGHT_AT`] and off the camera's axis
/// entirely, so nothing here has to clear it.
const SPOT_SHADOW_CAMERA_UP: f32 = 1.3;

/// [`Scene::SpotShadow`]'s caster: the pyramid, scaled and dropped so its base
/// sits exactly on the floor.
///
/// **On the floor rather than floating**, so the contact point of the shadow is
/// in the frame: a shadow detached from its caster is what too much depth bias
/// looks like, and a floating caster would hide that failure behind a gap that
/// is meant to be there.
fn spot_shadow_caster() -> glam::Mat4 {
    // The pyramid's base is at `-PYRAMID_HALF_BASE` in its own space, so lifting
    // it by that much of the scale puts the base on `y = 0`.
    glam::Mat4::from_translation(glam::Vec3::new(0.0, 0.4 * SPOT_SHADOW_CASTER_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(SPOT_SHADOW_CASTER_SCALE))
}

/// The camera [`Scene::SpotShadow`] is drawn with: straight down at the floor,
/// on [`spot_camera`]'s terms.
///
/// Overhead so the floor is a plane at a known scale and a pixel maps to a floor
/// position by a division — which is what lets the assertions name the band the
/// shadow falls in rather than searching for it.
fn spot_shadow_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, SPOT_SHADOW_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // floor's `+Z` axis at the top of the frame, which is the direction the
        // light comes *from* and so the direction the shadow falls away from.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// [`Scene::SpotShadow`]'s one light: a cone aimed at the floor's centre from
/// 45° up and behind it.
fn spot_shadow_light() -> crcbl_render::Light {
    crcbl_render::Light::Spot(crcbl_render::SpotLight {
        position: SPOT_SHADOW_LIGHT_AT,
        radius: SPOT_SHADOW_REACH,
        color: glam::Vec3::new(1.0, 0.95, 0.85) * SPOT_INTENSITY,
        // Along the cone, away from the light — so from the light to the floor's
        // centre, which is what makes the pool's axis land on the frame's.
        direction: -SPOT_SHADOW_LIGHT_AT,
        inner_angle: SPOT_SHADOW_INNER_ANGLE,
        outer_angle: SPOT_SHADOW_OUTER_ANGLE,
    })
}

/// How far above the floor [`Scene::PointShadow`] hangs its light.
///
/// **Low, and that is the scene.** A point light's six faces are picked by the
/// largest component of the direction from the light, so a receiver at `(x, -h)`
/// from a light `h` up is on the `-Y` face while `|x| < h` and on a side face
/// past that. A light high over a floor therefore puts the whole visible floor —
/// casters, shadows and all — on one face, and a frame drawn with every face
/// selection wired to that one would be identical. From here the four side faces
/// own everything past `|x| = h`, which is where both shadows fall.
///
/// Above the casters, or nothing casts anything; low enough that a caster a third
/// of its height throws a shadow several times its own length, which is what
/// makes the dark band wide enough to measure.
const POINT_LIGHT_UP: f32 = 0.5;

/// How far [`Scene::PointShadow`]'s light reaches, in world units.
///
/// Past the far end of both shadows, so the pool's edge is off the frame and
/// every band the assertions measure is lit by the same falloff — the claim is a
/// ratio between two bands equidistant from the light, and a radius cutting
/// through one of them would decide it.
const POINT_REACH: f32 = 3.0;

/// How far from the light's axis each of [`Scene::PointShadow`]'s casters
/// stands.
///
/// Past [`POINT_LIGHT_UP`], so a caster is already on the side face its shadow
/// falls across and the two are one face's business rather than a shadow
/// straddling a seam — the seam is real and untested here on purpose, and
/// `docs/backlog.md` is where that belongs.
const POINT_CASTER_AT: f32 = 0.6;

/// How much [`Scene::PointShadow`] scales the pyramid by to get each caster.
///
/// Short next to [`POINT_LIGHT_UP`]: the shadow's tip is at
/// `POINT_CASTER_AT * up / (up - height)`, which runs away to infinity as a
/// caster approaches the light's own height. At this scale the tip lands
/// comfortably inside the frame.
const POINT_CASTER_SCALE: f32 = 0.3;

/// How far above the floor [`Scene::PointShadow`]'s camera stands.
///
/// High enough that both shadows and both of their mirror bands are inside the
/// frame: the far one reaches about `1.3` from the centre and the frame's short
/// half-axis on the floor is `up * tan(30°)`.
const POINT_CAMERA_UP: f32 = 2.2;

/// The camera [`Scene::PointShadow`] is drawn with: straight down at the floor,
/// on [`spot_camera`]'s terms exactly.
fn point_shadow_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, POINT_CAMERA_UP, 0.0),
        target: glam::Vec3::ZERO,
        // `Y` is the view direction, so `up` cannot also be `Y`; `+Z` puts the
        // world's `+Z` axis at the top of the frame.
        up: glam::Vec3::Z,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// [`Scene::PointShadow`]'s one light: a point light just above the floor's
/// centre.
///
/// Near-white, on [`spot_light`]'s terms: the floor's albedo is the cube's green
/// `+Y` face and a coloured light would tint the bands the assertions compare
/// rather than leaving the comparison to the shadow.
fn point_shadow_light() -> crcbl_render::Light {
    crcbl_render::Light::Point(crcbl_render::PointLight {
        position: glam::Vec3::new(0.0, POINT_LIGHT_UP, 0.0),
        radius: POINT_REACH,
        color: glam::Vec3::new(1.0, 0.95, 0.85) * SPOT_INTENSITY,
    })
}

/// One of [`Scene::PointShadow`]'s casters: the pyramid, scaled and dropped so
/// its base sits on the floor, at `offset` from the light's axis.
///
/// **On the floor rather than floating**, on `spot_shadow_caster`'s terms: a
/// shadow detached from its caster is what too much depth bias looks like, and a
/// floating caster hides that behind a gap that is meant to be there.
fn point_shadow_caster(offset: glam::Vec3) -> glam::Mat4 {
    // The pyramid's base is at `-0.4` in its own space, so lifting it by that
    // much of the scale puts the base on `y = 0`.
    glam::Mat4::from_translation(offset + glam::Vec3::new(0.0, 0.4 * POINT_CASTER_SCALE, 0.0))
        * glam::Mat4::from_scale(glam::Vec3::splat(POINT_CASTER_SCALE))
}

/// The colour [`Scene::Sprite`] and [`Scene::Ui`] composite onto, in **linear**
/// light — which is what a clear value on an sRGB attachment means.
///
/// Neither pass clears: both load what is already in the target, because
/// compositing onto it is what they are for. So the scene clears first, and
/// deliberately not to black — alpha blending onto black is the one background
/// where `src * a + dst * (1 - a)` and `src * a` agree, so a premultiplication
/// mistake would be invisible in exactly the frame meant to reveal it. Same
/// value, and the same reasoning, as `crcbl-vk`'s sprite suite.
const SCENE_CLEAR: [f32; 4] = [0.10, 0.20, 0.35, 1.0];

/// Half the world height [`Scene::Sprite`]'s orthographic camera shows.
///
/// The frame size is the caller's (`--size`), so the scene is laid out in world
/// units and the projection scales it: every rectangle below sits inside
/// `|x| <= 95`, which is on screen for any frame at least as wide as it is tall.
const SPRITE_HALF_HEIGHT: f32 = 100.0;

/// [`Scene::Sprite`]'s first sheet: 4×2 texels, two 2×2 frames side by side,
/// and no two texels alike.
///
/// ```text
///   frame A          frame B
///   red    green  |  cyan   magenta
///   blue   yellow |  white  black
/// ```
///
/// Asymmetric for the reason `crcbl-vk`'s sprite suite records: a flipped V
/// swaps red with blue, a flipped U swaps red with green, and a symmetric test
/// image passes through both while looking entirely plausible.
const SPRITE_SHEET_A: [u8; 32] = [
    255, 0, 0, 255, // A top-left: red
    0, 255, 0, 255, // A top-right: green
    0, 255, 255, 255, // B top-left: cyan
    255, 0, 255, 255, // B top-right: magenta
    0, 0, 255, 255, // A bottom-left: blue
    255, 255, 0, 255, // A bottom-right: yellow
    255, 255, 255, 255, // B bottom-left: white
    0, 0, 0, 255, // B bottom-right: black
];

/// [`Scene::Sprite`]'s second sheet: 2×2 texels in four colours that appear
/// nowhere in [`SPRITE_SHEET_A`], so "which sheet was bound" is readable
/// straight off the picture.
const SPRITE_SHEET_B: [u8; 16] = [
    255, 128, 0, 255, // orange
    128, 0, 255, 255, // violet
    0, 128, 128, 255, // teal
    128, 128, 128, 255, // grey
];

/// The tint on the fourth sprite.
///
/// A different factor per channel, because a tint that left any channel alone
/// would let the tinted rectangle share colours with the untinted one it is
/// there to be told apart from.
const SPRITE_TINT: [f32; 4] = [0.5, 0.7, 0.9, 1.0];

/// Frame A of [`SPRITE_SHEET_A`], as normalised UVs.
const SPRITE_FRAME_A: [f32; 4] = [0.0, 0.0, 0.5, 1.0];
/// Frame B of [`SPRITE_SHEET_A`].
const SPRITE_FRAME_B: [f32; 4] = [0.5, 0.0, 1.0, 1.0];

/// [`Scene::Sprite`]'s four rectangles, in world units: `[x, y, w, h]`, minimum
/// corner first, Y up. Ten units apart, so no two of them touch.
const SPRITE_RECTS: [[f32; 4]; 4] = [
    [-95.0, -20.0, 40.0, 40.0],
    [-45.0, -20.0, 40.0, 40.0],
    [5.0, -20.0, 40.0, 40.0],
    [55.0, -20.0, 40.0, 40.0],
];

/// Which sheet each of [`SPRITE_RECTS`] samples: **A A B A**, not A A B B.
///
/// The submission order of a `&[Sprite]` is the batching, so this is three
/// batches over two sheets and the third batch starts at instance 3. That
/// arrangement is the whole point of the scene: one batch is exactly the case
/// that hid the `SV_InstanceID` divergence, because with a single draw the
/// SPIR-V and WGSL lowerings of the instance index agree. A backend that reads
/// the wrong one draws the last rectangle in the *first* rectangle's place,
/// leaving its own slot at the clear colour.
const SPRITE_ORDER: [usize; 4] = [0, 0, 1, 0];

/// How far past the patch's near edge [`Scene::Dunes`]'s camera stands, and how
/// far up.
///
/// The same arrangement `crcbl-vk`'s `dunes_camera` uses and
/// `crcbl_shaders::cluster_dag`'s receding-patch test reads its histograms from:
/// the patch is centred on the origin in `x` and `z` with its height on `y`, so
/// an eye at negative `z` and a small `y` is a viewer standing at one end of a
/// ground plane whose far edge is tens of times further away than its near one.
const DUNES_CAMERA_BACK: f32 = 2.0;
const DUNES_CAMERA_UP: f32 = 4.0;

/// The camera [`Scene::Dunes`] is drawn with.
///
/// **Perspective**, and that is not decoration: the metric divides a projected
/// error by the distance to a group's sphere, an orthographic projection has no
/// such falloff, and `ForwardRenderer` answers that honestly by selecting the
/// base level whole. A scene meant to show a level being *chosen* has to have a
/// distance term to choose by.
fn dunes_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(
            0.0,
            DUNES_CAMERA_UP,
            -crcbl_shaders::dunes::DUNES_EXTENT - DUNES_CAMERA_BACK,
        ),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: Projection::Perspective {
            fov_y: std::f32::consts::FRAC_PI_3,
            near: 0.01,
        },
    }
}

/// The camera [`Scene::Sprite`] is drawn with: orthographic, looking down −Z at
/// the plane the sprites live on.
fn sprite_camera() -> Camera {
    Camera {
        eye: glam::Vec3::new(0.0, 0.0, 1.0),
        target: glam::Vec3::ZERO,
        up: glam::Vec3::Y,
        projection: Projection::Orthographic {
            half_height: SPRITE_HALF_HEIGHT,
            near: 0.1,
            far: 10.0,
        },
    }
}

/// [`Scene::Sprite`]'s four sprites, over the two registered sheets.
fn sprite_scene(sheets: [SheetId; 2]) -> [Sprite; 4] {
    let uv = |index: usize| match index {
        0 => SPRITE_FRAME_A,
        1 => SPRITE_FRAME_B,
        // The second sheet has one frame, which is the whole image.
        2 => [0.0, 0.0, 1.0, 1.0],
        _ => SPRITE_FRAME_A,
    };
    std::array::from_fn(|index| {
        let sprite = Sprite::new(sheets[SPRITE_ORDER[index]], SPRITE_RECTS[index], uv(index));
        // Only the last one is tinted, so the two rectangles that share frame A
        // are the same picture in different colours.
        if index == 3 {
            sprite.with_tint(SPRITE_TINT)
        } else {
            sprite
        }
    })
}

/// [`Scene::Ui`]'s draw list, in the pass's Y-down screen pixels.
///
/// Laid out as fractions of `extent` so the same picture arrives at every
/// `--size`, and built from all three command kinds: a filled rectangle, a
/// translucent one straddling its edge, an outline, and two lines of text
/// through the glyph atlas. Text alone would leave a broken atlas binding
/// looking like an empty frame; a rectangle alone would never sample the atlas
/// at all.
fn ui_draw_list(extent: (u32, u32)) -> DrawList {
    use glam::Vec2;

    let (width, height) = (extent.0 as f32, extent.1 as f32);
    let at = |x: f32, y: f32| Vec2::new(width * x, height * y);

    let mut list = DrawList::new();
    // The panel, then the translucent bar half on it and half off it: the two
    // colours it blends to are two more distinct colours in the frame, and they
    // are the only evidence the pass blends rather than replaces.
    list.rect(at(0.08, 0.10), at(0.92, 0.62), [0.15, 0.20, 0.55, 1.0]);
    list.rect(at(0.30, 0.45), at(0.70, 0.85), [1.0, 0.45, 0.10, 0.5]);
    list.rect_outline(
        at(0.03, 0.04),
        at(0.97, 0.96),
        (height * 0.02).max(1.0),
        [1.0, 0.85, 0.0, 1.0],
    );
    list.text(at(0.12, 0.16), "CRCBL", [1.0, 1.0, 1.0, 1.0], height * 0.18);
    list.text(
        at(0.12, 0.66),
        "ui scene",
        [0.2, 1.0, 0.4, 1.0],
        height * 0.10,
    );
    list
}

/// The renderer, and the content, for the scene being drawn.
///
/// One variant per [`Scene`]; the frame's per-scene work is the two arms of
/// [`OffscreenSetup::draw_and_readback`] and nothing else keys off it.
enum SceneState {
    /// Every scene drawn through [`ForwardRenderer`]: one camera, one sun and
    /// one set of residents, differing in what is put in the scene and where the
    /// camera stands.
    Forward {
        camera: Camera,
        light: DirectionalLight,
        /// The cube's model matrix, which
        /// [`ForwardRenderer::begin_frame`](crate::render::ForwardRenderer::begin_frame)
        /// takes as an argument rather than off a setter. Every scene but
        /// [`Scene::Spot`] leaves it at `ForwardRenderer::spin(0.0)`, which is
        /// the identity; that one scales it into a floor.
        model: glam::Mat4,
        /// Boxed because it is much the largest of the three: it carries the
        /// geometry pools and the instance ring, and an unboxed variant would
        /// make every `SceneState` — including the two small ones — that size.
        renderer: Box<ForwardRenderer>,
    },
    Sprite {
        renderer: SpriteRenderer,
        sheets: [SheetId; 2],
    },
    Ui {
        renderer: UiRenderer,
        atlas: FontAtlas,
    },
}

impl SceneState {
    /// What this scene's renderer recorded for the frame just drawn.
    ///
    /// One renderer per scene, so this is that renderer's own
    /// [`counters`](ForwardRenderer::counters) rather than a sum — a game adds
    /// up several, and this module draws exactly one thing.
    fn counters(&self) -> FrameCounters {
        match self {
            Self::Forward { renderer, .. } => renderer.counters(),
            Self::Sprite { renderer, .. } => renderer.counters(),
            Self::Ui { renderer, .. } => renderer.counters(),
        }
    }

    /// Builds the renderer this scene needs, and uploads whatever it draws.
    fn open(
        scene: Scene,
        device: &dyn Device,
        queue: QueueHandle,
        format: Format,
    ) -> Result<Self, OffscreenError> {
        Ok(match scene {
            Scene::Cube => {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_pyramid(Some(glam::Mat4::from_translation(PYRAMID_AT)));
                renderer.set_tinted_pyramid(Some(glam::Mat4::from_translation(TINTED_PYRAMID_AT)));
                renderer
                    .set_textured_pyramid(Some(glam::Mat4::from_translation(TEXTURED_PYRAMID_AT)));
                Self::Forward {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: DirectionalLight::default(),
                    model: ForwardRenderer::spin(0.0),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Lights => {
                // The cube scene's geometry exactly, so the two goldens differ
                // in their light lists and in nothing else.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_pyramid(Some(glam::Mat4::from_translation(PYRAMID_AT)));
                renderer.set_tinted_pyramid(Some(glam::Mat4::from_translation(TINTED_PYRAMID_AT)));
                renderer
                    .set_textured_pyramid(Some(glam::Mat4::from_translation(TEXTURED_PYRAMID_AT)));
                renderer.set_lights(&scene_lights());
                Self::Forward {
                    camera: Camera::default().with_projection(Projection::Perspective {
                        fov_y: std::f32::consts::FRAC_PI_3,
                        near: 0.01,
                    }),
                    light: dim_sun(),
                    model: ForwardRenderer::spin(0.0),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Spot => {
                // **The cube alone, and it is the floor.** Every other resident
                // stays off: a pyramid beside the pool would be a second lit
                // shape in a frame whose whole content is meant to be one cone
                // on one flat surface.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_lights(&[spot_light()]);
                Self::Forward {
                    camera: spot_camera(),
                    light: spot_sun(),
                    model: spot_floor(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::SpotShadow => {
                // The spot scene's floor exactly, with one thing added: the
                // pyramid, standing in the light. Everything else that differs
                // — the camera, the light's tilt — is what makes the shadow
                // separable from its caster, and `Scene::SpotShadow` is where
                // that is argued.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_pyramid(Some(spot_shadow_caster()));
                renderer.set_lights(&[spot_shadow_light()]);
                Self::Forward {
                    camera: spot_shadow_camera(),
                    light: spot_sun(),
                    model: spot_floor(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::PointShadow => {
                // The spot-shadow scene's floor, with **two** casters on
                // different sides of the light: one out along `+X` and one out
                // along `-Z`, so their shadows fall across two different faces
                // of the light's map. One caster would prove a point light casts
                // *a* shadow, which is what a single working face already does.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_pyramid(Some(point_shadow_caster(glam::Vec3::new(
                    POINT_CASTER_AT,
                    0.0,
                    0.0,
                ))));
                renderer.set_tinted_pyramid(Some(point_shadow_caster(glam::Vec3::new(
                    0.0,
                    0.0,
                    -POINT_CASTER_AT,
                ))));
                renderer.set_lights(&[point_shadow_light()]);
                Self::Forward {
                    camera: point_shadow_camera(),
                    light: spot_sun(),
                    model: spot_floor(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Dunes => {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                // **Refused rather than drawn empty.** `set_dunes` says no on a
                // device that reports a mesh stage and no amplification stage,
                // where the un-amplified path would emit every cluster of every
                // level at once; every other device draws the patch, per cluster
                // or through a uniform cut. Ignoring the answer is what made
                // this scene a frame of clear colour that a golden would have
                // been blessed from.
                if !renderer.set_dunes(Some(glam::Mat4::IDENTITY)) {
                    renderer.destroy(device);
                    return Err(OffscreenError::Unusable(
                        "this device reports a mesh stage and no amplification stage, so the \
                         dunes patch's cluster DAG has nothing that can select a level in it",
                    ));
                }
                Self::Forward {
                    camera: dunes_camera(),
                    light: DirectionalLight::default(),
                    model: ForwardRenderer::spin(0.0),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Sprite => {
                let mut renderer = SpriteRenderer::new(device, queue, format)?;
                let mut register = |label, width, height, pixels| {
                    renderer.register_sheet(
                        device,
                        &SheetDesc {
                            label,
                            width,
                            height,
                            // Pixel art's sampler, and the branch of
                            // `sprite.slang` a game actually ships on.
                            sample: SampleMode::Pixel,
                            pixels,
                        },
                    )
                };
                let sheets = match (
                    register("screenshot sheet A", 4, 2, &SPRITE_SHEET_A),
                    register("screenshot sheet B", 2, 2, &SPRITE_SHEET_B),
                ) {
                    (Ok(a), Ok(b)) => [a, b],
                    // Whichever failed, the renderer owns everything that did
                    // upload and gives it back here rather than at drop.
                    (Err(error), _) | (Ok(_), Err(error)) => {
                        renderer.destroy(device);
                        return Err(OffscreenError::Hal(error));
                    }
                };
                Self::Sprite { renderer, sheets }
            }
            Scene::Ao => {
                // The open box alone, and the cube parked out of frame — see
                // `ao_parked_cube`. Every other resident stays off for
                // `Scene::Spot`'s reason: what this frame is about is one
                // concave corner and the flat floor beside it.
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer.set_open_box(Some(ao_box()));
                Self::Forward {
                    camera: ao_camera(),
                    light: ao_sun(),
                    model: ao_parked_cube(),
                    renderer: Box::new(renderer),
                }
            }
            Scene::Ui => Self::Ui {
                renderer: UiRenderer::new(device, queue, format)?,
                atlas: FontAtlas::built_in(),
            },
        })
    }

    /// Releases the scene's GPU resources. The device must be idle.
    fn destroy(self, device: &dyn Device) {
        match self {
            Self::Forward { renderer, .. } => renderer.destroy(device),
            Self::Sprite { renderer, .. } => renderer.destroy(device),
            Self::Ui { renderer, .. } => renderer.destroy(device),
        }
    }
}

// ---------------------------------------------------------------------------
// OffscreenSetup
// ---------------------------------------------------------------------------

/// The largest edge, in pixels, an offscreen frame may have.
///
/// 16384 is `maxImageDimension2D` on every implementation the engine targets,
/// so anything past it would be refused by swapchain creation anyway — but
/// only *after* this module had already tried to allocate a host-visible
/// staging buffer for it. `16384x16384` RGBA8 is one gibibyte, which is the
/// most this path will ever ask an allocator for.
///
/// The CLI checks `--size` against this at parse time so an absurd request is
/// a bad *invocation* (exit 2) rather than a failed command; [`OffscreenSetup::open`]
/// checks it again because a library may not have gone through the CLI.
pub const MAX_DIMENSION: u32 = 16_384;

/// Row-pitch alignment, in bytes, for the readback staging buffer.
///
/// **Not a performance hint — a portability requirement.** wgpu refuses a
/// multi-row buffer↔image copy whose row pitch is not a multiple of
/// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256), so a tightly packed readback —
/// which is legal on Vulkan and is what this module used to record — fails on
/// `crcbl-wgpu` for every width that is not a multiple of 64:
///
/// ```text
/// $ CRCBL_GPU=wgpu crcbl screenshot --size 32x32 --output /tmp/x.png
/// crcbl: render/readback failed: HAL: invalid descriptor: a buffer↔image copy
///   of 32 texel(s) per row is 128 bytes, which wgpu requires to be a multiple
///   of 256
/// ```
///
/// Found by the cross-backend harness at P5.12, which compares this path's
/// output between the two backends at more than one frame size — `256x192`, the
/// only size anything had ever asked for, happens to be a multiple of 64 and hid
/// it. Vulkan imposes no such rule and pads harmlessly, so the padded pitch is
/// unconditional rather than a backend-specific branch: nothing above
/// `crcbl-hal` may key off which backend is behind the seam.
///
/// The padding never reaches the caller — [`OffscreenSetup::draw_and_readback`]
/// compacts the rows before returning.
pub const READBACK_ROW_ALIGNMENT: u32 = 256;

/// How long [`OffscreenSetup::draw_and_readback`] waits for the copy to land.
///
/// Generous because an offscreen ring on a software rasteriser can take
/// hundreds of milliseconds for a single frame. Public because the two error
/// paths that mention it are public, and a deadline a caller can be hit by is
/// one it should be able to read.
pub const READBACK_DEADLINE: Duration = Duration::from_secs(10);

/// How many images the offscreen ring holds.
///
/// More than one so the path a windowed swapchain takes — acquire a *different*
/// image, present, come round again — is the path a screenshot takes too. It is
/// named rather than written into the descriptor because the barrier test below
/// has to draw more frames than this to reach a re-used image at all, and a lap
/// count derived from a literal two files away is one that silently stops
/// meaning what it says.
const RING_IMAGES: u32 = 2;

/// Holds everything needed to render one frame offscreen: a GPU instance,
/// device, offscreen swapchain ring, and the chosen scene's renderer.
///
/// The caller drives one frame via [`Self::draw_and_readback`], then tears
/// down with [`Self::finish`].
#[allow(missing_debug_implementations)]
pub struct OffscreenSetup {
    instance: Box<dyn Instance>,
    device: Box<dyn Device>,
    surface: SurfaceHandle,
    swapchain: SwapchainHandle,
    queue: QueueHandle,
    format: Format,
    /// The adapter the device was created on, kept so [`Self::adapter`] can
    /// answer after the enumeration it came from has been dropped.
    adapter: crate::hal::AdapterInfo,
    /// The renderer for the [`Scene`] this setup was opened with, and whatever
    /// that scene draws.
    scene: SceneState,
    pool: TransientPool,
}

/// Reasons an offscreen render might fail before a pixel is written.
#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    /// No GPU backend could be opened.
    #[error("GPU backend: {0}")]
    Backend(#[from] crate::backend::GpuError),

    /// No adapter, no queue, no format, or a surface-cap query failed.
    #[error("device not usable: {0}")]
    Unusable(&'static str),

    /// [`ADAPTER_ENV_VAR`](crate::adapter::ADAPTER_ENV_VAR) named a device class
    /// this backend's enumeration does not have exactly one of, or a word that
    /// is not a class at all.
    ///
    /// Never a fallback: a frame drawn on an adapter nobody asked for is a green
    /// run that is evidence about the wrong device.
    #[error("{0}")]
    AdapterPin(#[from] crate::adapter::PinMiss),

    /// A HAL call failed.
    #[error("HAL: {0}")]
    Hal(#[from] crate::hal::HalError),

    /// A surface operation failed.
    #[error("surface: {0}")]
    Surface(#[from] crate::hal::SurfaceError),

    /// A graph compile or execute failed.
    #[error("graph: {0}")]
    Graph(#[from] GraphError),

    /// The swapchain went out of date before the first frame completed.
    #[error("offscreen swapchain is out of date")]
    OutOfDate,

    /// The requested frame is larger than [`MAX_DIMENSION`] on an edge, or its
    /// byte count does not fit this machine's address space.
    #[error("{width}x{height} is larger than the {MAX_DIMENSION}x{MAX_DIMENSION} offscreen limit")]
    TooLarge {
        /// Requested width, in pixels.
        width: u32,
        /// Requested height, in pixels.
        height: u32,
    },

    /// The readback did not land within [`READBACK_DEADLINE`].
    ///
    /// A `Result` rather than a panic because the CLI's contract is exit 1 with
    /// a `--json`-shaped message, and a `panic!` here aborted with exit 101
    /// past `report::emit` entirely.
    #[error("readback did not complete within {0:?}")]
    ReadbackTimeout(Duration),
}

impl OffscreenSetup {
    /// What [`Self::open`] asks the device for, optionally.
    ///
    /// [`Features::MESH_SHADER`] is in here because
    /// `docs/plan/03-gpu-driven-rendering.md` §3.5 makes the mesh path the
    /// **primary** geometry path and a device is only on it if something asked:
    /// the flag is not part of [`Features::GPU_DRIVEN`] — that bundle is the
    /// data-layout axis, and folding a second selector into it would make it a
    /// tier again — so it has to be named beside it. Every scene here draws
    /// identically on either path, which `tests/render_e2e.rs` checks by drawing
    /// each one twice through [`Self::open_with`].
    ///
    /// Optional, never required: a device without any of these opens and draws
    /// the same picture through a lesser tail.
    pub const OPTIONAL_FEATURES: Features = Features::GPU_DRIVEN
        .union(Features::MESH_SHADER)
        // **Not implied by `MESH_SHADER`**, and asked for separately because
        // `ForwardRenderer` builds §3.5's amplification stage only where the
        // device has one — and without it `mesh_cluster.slang`'s un-amplified
        // `meshMain` emits every cluster of every level, which
        // [`Scene::Dunes`]' hierarchy cannot survive. A mesh adapter that has it
        // and a setup that did not ask is the same defect this list already
        // records for `MESH_SHADER`: the frame comes out of a lesser path and
        // nothing says so.
        .union(Features::TASK_SHADER)
        .union(Features::DEBUG_MARKERS);

    /// Opens the auto-selected GPU backend, creates an offscreen surface,
    /// adapter, device, swapchain, and `scene`'s renderer for a frame of
    /// `(width, height)` pixels.
    ///
    /// [`Scene::default`] is [`Scene::Cube`], the frame this module drew before
    /// there was anything to choose between.
    ///
    /// Which backend is [`crate::backend`]'s decision and which adapter inside
    /// it is [`crate::adapter`]'s; [`Self::backend`] and [`Self::adapter`]
    /// report what both of them answered.
    ///
    /// Returns `Err` if the frame is not between `1x1` and
    /// [`MAX_DIMENSION`]`x`[`MAX_DIMENSION`], if no GPU is available (lavapipe,
    /// swiftshader, or a real card), if
    /// [`ADAPTER_ENV_VAR`](crate::adapter::ADAPTER_ENV_VAR) names no adapter
    /// this backend enumerated, if the device is unusable, or if any HAL call
    /// fails.
    pub fn open(width: u32, height: u32, scene: Scene) -> Result<Self, OffscreenError> {
        Self::open_with(width, height, scene, Self::OPTIONAL_FEATURES)
    }

    /// [`Self::open`] asking the device for `optional_features` instead of
    /// [`Self::OPTIONAL_FEATURES`].
    ///
    /// The features are optional here for the same reason they are there: a
    /// device that lacks one still opens and still draws. What this adds is the
    /// ability to open a device *without* something the adapter has, which is
    /// the only way a caller on one machine can reach more than one
    /// [`GeometryPath`](crate::hal::GeometryPath) — every path but the best one
    /// the adapter reports is otherwise code no run here executes. `crcbl-vk`'s
    /// `Headless::open_for_mesh_with` is the same knob one layer down, and
    /// `tests/render_e2e.rs` uses this one to draw each scene twice and compare.
    ///
    /// # Errors
    ///
    /// The same as [`Self::open`].
    pub fn open_with(
        width: u32,
        height: u32,
        scene: Scene,
        optional_features: Features,
    ) -> Result<Self, OffscreenError> {
        // Checked before the backend is opened, so an absurd `--size` costs a
        // comparison rather than a device.
        if width == 0 || height == 0 {
            return Err(OffscreenError::Unusable("a frame must be at least 1x1"));
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(OffscreenError::TooLarge { width, height });
        }

        Self::open_on(
            crate::backend::open()?,
            width,
            height,
            scene,
            optional_features,
        )
    }

    /// [`Self::open`] on an instance that has already been opened.
    ///
    /// The split exists so the barrier test below can drive the whole of
    /// [`Self::draw_and_readback`] against `crcbl_hal::null`, whose recorder is
    /// the only thing in the tree that can be *asked* what command stream this
    /// module produced. The size checks stay in [`Self::open`]: they are about
    /// the caller's `--size`, and refusing before a backend is opened is the
    /// property their test asserts.
    fn open_on(
        instance: Box<dyn Instance>,
        width: u32,
        height: u32,
        scene: Scene,
        optional_features: Features,
    ) -> Result<Self, OffscreenError> {
        let extent = (width, height);

        let target = SurfaceTarget::Offscreen;
        // SAFETY: `Offscreen` names no platform object, so nothing can dangle.
        let surface = unsafe {
            instance
                .create_surface(&target)
                .map_err(OffscreenError::Hal)?
        };

        let adapters = instance.adapters();
        // Not `.first()`: the first enumerated adapter is not a device that
        // works on every machine, and a frame drawn on one nobody named is not
        // evidence. See [`crate::adapter`] for the measurement that moved this.
        let adapter = crate::adapter::select(crate::adapter::pin().as_deref(), &adapters)?;

        let caps = instance
            .surface_caps(surface, adapter.id)
            .map_err(OffscreenError::Hal)?;

        let format = caps
            .preferred_format()
            .ok_or(OffscreenError::Unusable("no surface format"))?;

        let device = instance
            .create_device(&DeviceDesc {
                label: Some("crcbl screenshot"),
                adapter: adapter.id,
                required_features: Features::empty(),
                optional_features,
                compatible_surface: Some(surface),
            })
            .map_err(OffscreenError::Hal)?;

        let queue = device
            .queue(QueueKind::Graphics)
            .ok_or(OffscreenError::Unusable("no graphics queue"))?;

        let swapchain = device
            .create_swapchain(&SwapchainDesc {
                label: Some("screenshot ring"),
                surface,
                format,
                extent,
                image_count: RING_IMAGES,
                present_mode: PresentMode::Fifo,
                composite_alpha: crate::hal::CompositeAlpha::Opaque,
            })
            .map_err(OffscreenError::Surface)?;

        let scene = SceneState::open(scene, device.as_ref(), queue, format)?;

        Ok(Self {
            instance,
            device,
            surface,
            swapchain,
            queue,
            format,
            adapter: adapter.clone(),
            scene,
            pool: TransientPool::new(),
        })
    }

    /// The surface format the readback bytes are in.
    ///
    /// The swapchain's preferred format is `Bgra8UnormSrgb` on most surfaces,
    /// and [`Self::draw_and_readback`] copies the swapchain image *raw*. A
    /// caller turning those bytes into an image therefore has to know whether
    /// to swizzle red and blue, and this is how it knows. Feeding them to an
    /// RGBA constructor unconditionally produces a channel-swapped PNG on
    /// every ordinary desktop surface.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Which backend [`crate::backend::open`] selected for this frame.
    ///
    /// The frame itself cannot say: every backend renders the same scene
    /// through the same [`ForwardRenderer`], so a caller comparing pixels has
    /// no way to tell which one produced them. A test that pinned
    /// [`BACKEND_ENV_VAR`](crate::backend::BACKEND_ENV_VAR) and never checked
    /// would therefore pass identically on the backend it asked for and on the
    /// one it fell back to — "not supported here" arriving as a green run.
    #[must_use]
    pub fn backend(&self) -> crate::hal::BackendKind {
        self.device.backend()
    }

    /// Which adapter of that backend's enumeration the device was created on.
    ///
    /// The third of the same family as [`Self::backend`] and [`Self::caps`],
    /// and the one whose absence cost a CI run: this module took
    /// `adapters().first()` and said nothing, so a frame that died with
    /// `DXGI_ERROR_DEVICE_REMOVED` named neither the adapter it had opened nor
    /// the one it should have. A harness that pins
    /// [`ADAPTER_ENV_VAR`](crate::adapter::ADAPTER_ENV_VAR) reads this back to
    /// check the pin landed — a variable that never reached the process and a
    /// pin that was honoured look identical from outside.
    #[must_use]
    pub const fn adapter(&self) -> &crate::hal::AdapterInfo {
        &self.adapter
    }

    /// What the opened device reported it can do.
    ///
    /// The selector this exists for is
    /// [`geometry_path`](crate::hal::DeviceCaps::geometry_path): the forward
    /// pass's indirect tail is chosen from it once, at build, and is otherwise
    /// invisible from outside — so this is how a caller learns which arm a
    /// frame was actually drawn through rather than assuming the one its
    /// developer's GPU happens to select.
    #[must_use]
    pub fn caps(&self) -> crate::hal::DeviceCaps {
        self.device.caps()
    }

    /// What the last [`draw_and_readback`](Self::draw_and_readback) recorded,
    /// and what the GPU has told it since.
    ///
    /// The same [`FrameCounters`] a game puts on its debug panel, which is what
    /// makes it worth asserting here: a headless run is the only place a
    /// cross-backend test can watch the culling counters come back off the GPU,
    /// and `crcbl_render::cull_stats`' ring means they arrive several frames
    /// after the frame they describe — see
    /// [`FrameCounters::cull_frame`](crate::render::FrameCounters::cull_frame),
    /// which says which frame that was.
    #[must_use]
    pub fn counters(&self) -> FrameCounters {
        self.scene.counters()
    }

    /// Records, submits, and reads back one frame.
    ///
    /// Returns the swapchain image's bytes as `((width, height), Vec<u8>)`,
    /// four bytes per pixel, row-major, top row first, in [`Self::format`]'s
    /// channel order — **not** necessarily RGBA.
    ///
    /// The pose is fixed at `t = 0`: a screenshot is a golden-image input, and
    /// a deterministic frame is the only kind worth comparing against a
    /// reference. (There was an `advance`/`elapsed` pair here to move the
    /// clock; nothing ever called it, so every screenshot rendered `t = 0`
    /// anyway and the state only made the frame look configurable.)
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if recording, submission, or readback fail,
    /// [`OffscreenError::OutOfDate`] if the swapchain is stale,
    /// [`OffscreenError::TooLarge`] if the acquired extent's byte count does
    /// not fit in a `usize`, or [`OffscreenError::ReadbackTimeout`] if the copy
    /// has not landed after [`READBACK_DEADLINE`].
    pub fn draw_and_readback(&mut self) -> Result<((u32, u32), Vec<u8>), OffscreenError> {
        let device = self.device.as_ref();
        let acquired = device
            .acquire_next_frame(self.swapchain)
            .map_err(|error| match error {
                SurfaceError::OutOfDate => OffscreenError::OutOfDate,
                other => OffscreenError::Surface(other),
            })?;

        let extent = acquired.extent;
        // The extent comes back from the swapchain rather than from `open`, so
        // it is checked here too: `u32 * u32 * 4` overflows a `u32` and the
        // product has to survive narrowing to a `usize` before it can size a
        // staging buffer or a `Vec`.
        let too_large = || OffscreenError::TooLarge {
            width: extent.0,
            height: extent.1,
        };
        // The *staged* row pitch, padded to `READBACK_ROW_ALIGNMENT`; the
        // tightly packed pitch is what the caller gets back.
        let packed_pitch = u64::from(extent.0).checked_mul(4).ok_or_else(too_large)?;
        let staged_pitch = packed_pitch
            .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
            .ok_or_else(too_large)?;
        let byte_count = staged_pitch
            .checked_mul(u64::from(extent.1))
            .ok_or_else(too_large)?;
        let packed_bytes = packed_pitch
            .checked_mul(u64::from(extent.1))
            .ok_or_else(too_large)?;
        let staged_capacity = usize::try_from(byte_count).map_err(|_| too_large())?;
        let host_capacity = usize::try_from(packed_bytes).map_err(|_| too_large())?;
        // `buffer_row_length` is in *texels*, and the padded pitch is a multiple
        // of 4 for every 4-byte format, so this division is exact.
        let staged_row_texels = u32::try_from(staged_pitch / 4).map_err(|_| too_large())?;

        // ---- render the frame through the graph ----

        let compiled = {
            let mut graph = RenderGraph::new(self.queue);
            // The same swapchain import for every scene: what differs between
            // them is which passes are hung off it, not how it is presented.
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, self.format, extent),
            );
            match &mut self.scene {
                SceneState::Forward {
                    camera,
                    light,
                    model,
                    renderer,
                } => {
                    renderer.begin_frame(device, camera, light, *model, extent)?;
                    let _hdr = renderer.add_passes(&mut graph, target, extent);
                }
                SceneState::Sprite { renderer, sheets } => {
                    let sprites = sprite_scene(*sheets);
                    let aspect = extent.0 as f32 / extent.1 as f32;
                    renderer.begin_frame(
                        device,
                        &sprites,
                        sprite_camera().view_projection(aspect),
                        extent,
                    )?;
                    // Both of the passes below load rather than clear, so the
                    // scene supplies the background they composite onto.
                    graph
                        .add_render_pass("scene background")
                        .clear_color(target, SCENE_CLEAR)
                        .execute(|_| {});
                    renderer.add_pass(&mut graph, target);
                }
                SceneState::Ui { renderer, atlas } => {
                    // `scale` is 1.0 because every size in the draw list is
                    // already a fraction of this frame's extent; a second
                    // multiplier is a second thing that can disagree with it.
                    renderer.begin_frame(device, &ui_draw_list(extent), atlas, 1.0)?;
                    graph
                        .add_render_pass("scene background")
                        .clear_color(target, SCENE_CLEAR)
                        .execute(|_| {});
                    renderer.add_pass(&mut graph, target, extent);
                }
            }
            graph.compile(&self.pool)?
        };

        let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
            label: Some("screenshot frame"),
            queue: self.queue,
        });
        compiled.execute(device, &mut self.pool, encoder.as_mut(), None)?;

        // ---- readback: barrier to TransferSrc, copy, barrier back, submit ----
        //
        // **Both ends of this pair are `ResourceState::Present`, not
        // `ColorAttachment`.** The graph does not hand the image back in the
        // state its last pass left it in: `ForwardRenderer::present_target`
        // declares `final_state: Present`, and `CompiledGraph::execute` emits a
        // trailing barrier to reach it. So `Present` is what the image is in
        // when the copy starts, and declaring anything else is a lie the API
        // checks — lavapipe's validation layer reported this one as
        // `VUID-VkImageMemoryBarrier2-oldLayout-01197`, "cannot transition …
        // from VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL when the previous known
        // layout is VK_IMAGE_LAYOUT_PRESENT_SRC_KHR".
        //
        // And the second barrier is why there is a pair at all. `present` takes
        // the image back into the ring, and the next trip round declares
        // `Undefined` — legal from any layout on Vulkan, but on D3D12
        // `Undefined` and `Present` are both `COMMON` and the declared
        // before-state is validated, so an image left in `COPY_SOURCE` makes
        // that next declaration false. `crcbl-dx12`'s own offscreen-ring suite
        // ends every frame with this same transition for that reason.

        let staging = device.create_buffer(&BufferDesc {
            label: Some("screenshot readback"),
            size: byte_count,
            usage: BufferUsage::TRANSFER_DST,
            memory: MemoryLocation::HostReadback,
        })?;

        let range = ImageSubresourceRange::all(self.format);
        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                range,
                ResourceState::Present,
                ResourceState::TransferSrc,
            )],
            ..Barriers::default()
        });

        encoder.copy_image_to_buffer(&BufferImageCopy {
            buffer: staging,
            buffer_offset: 0,
            buffer_row_length: staged_row_texels,
            buffer_image_height: 0,
            image: acquired.image,
            image_subresource: ImageSubresourceLayers {
                aspect: ImageAspect::COLOR,
                mip: 0,
                base_layer: 0,
                layer_count: 1,
            },
            image_offset: Offset3d::default(),
            image_extent: Extent3d::d2(extent.0, extent.1),
        });

        encoder.pipeline_barrier(&Barriers {
            images: &[ImageBarrier::new(
                acquired.image,
                range,
                ResourceState::TransferSrc,
                ResourceState::Present,
            )],
            ..Barriers::default()
        });

        let commands = encoder.finish()?;
        device.submit(self.queue, &SubmitInfo::new(&[commands]))?;
        device.present(
            self.queue,
            &PresentInfo {
                swapchain: self.swapchain,
                waits: acquired.present_semaphore.as_slice(),
                present_id: None,
            },
        )?;

        let readback = device.request_readback(&ReadbackDesc {
            label: Some("screenshot readback"),
            buffer: staging,
            offset: 0,
            size: byte_count,
            after: None,
        })?;

        let mut staged = vec![0u8; staged_capacity];

        let deadline = std::time::Instant::now() + READBACK_DEADLINE;
        loop {
            let state = device.poll_readback(readback, &mut staged)?;
            if let ReadbackState::Ready = state {
                break;
            }
            if std::time::Instant::now() > deadline {
                // The command buffer, staging buffer and readback are left
                // alone deliberately: the GPU may still be reading them, and
                // destroying them now would be worse than leaking them until
                // `finish` waits the device idle and drops it.
                return Err(OffscreenError::ReadbackTimeout(READBACK_DEADLINE));
            }
            std::thread::sleep(Duration::from_micros(100));
        }

        device.destroy_command_buffer(commands);
        device.destroy_buffer(staging);
        device.destroy_readback(readback);

        // Drop the row padding. Done here rather than left to the caller because
        // the pitch is this module's decision, and a caller that forgot it would
        // get a sheared image — the one failure a structural comparison sees and
        // a per-pixel one does not describe usefully.
        let pixels = compact_rows(&staged, staged_pitch, packed_pitch, host_capacity);

        Ok((extent, pixels))
    }

    /// Tears down in correct order: wait idle, destroy the scene and the
    /// transient pool, then swapchain → surface → device.
    ///
    /// # Errors
    ///
    /// [`OffscreenError::Hal`] if the device could not be brought to idle — a
    /// device lost during the frame surfaces here and nowhere else, and a
    /// caller about to save the pixels as a golden image needs to be told
    /// before it trusts them. The teardown still runs either way; the failure
    /// is reported after it.
    pub fn finish(mut self) -> Result<(), OffscreenError> {
        let idle = self.device.wait_idle();
        // After the wait, because both of these hand handles back to a device
        // that may still be reading them. `SpriteRenderer` and `UiRenderer` warn
        // on a drop that skipped this; `ForwardRenderer` and `TransientPool`
        // leak silently, which is why the screenshot path used to.
        self.scene.destroy(self.device.as_ref());
        self.pool.destroy(self.device.as_ref());
        self.device.destroy_swapchain(self.swapchain);
        self.instance.destroy_surface(self.surface);
        drop(self.device);
        drop(self.instance);
        idle.map_err(OffscreenError::Hal)
    }
}

/// Copies `packed_pitch` bytes out of every `staged_pitch`-byte row.
///
/// A free function so the arithmetic is testable with no GPU: this is the step
/// that turns a padded readback back into an image, and getting it wrong shears
/// the frame by a few pixels per row — which looks like a rendering bug and is
/// not one.
///
/// A short final row is copied as far as it goes rather than dropped: a backend
/// that wrote less than it promised should produce a visibly truncated image,
/// not a panic in the middle of a screenshot.
fn compact_rows(staged: &[u8], staged_pitch: u64, packed_pitch: u64, packed_len: usize) -> Vec<u8> {
    if staged_pitch == packed_pitch {
        let end = packed_len.min(staged.len());
        return staged[..end].to_vec();
    }
    // Both pitches sized a `Vec` above, so both fit a `usize`.
    let staged_pitch = staged_pitch as usize;
    let packed_pitch = packed_pitch as usize;
    let mut packed = Vec::with_capacity(packed_len);
    let mut offset = 0usize;
    while offset < staged.len() && packed.len() < packed_len {
        let row_end = (offset + packed_pitch).min(staged.len());
        packed.extend_from_slice(&staged[offset..row_end]);
        offset += staged_pitch;
    }
    packed.truncate(packed_len);
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The padding is only correct if it is dropped again, and this is the
    /// arithmetic that drops it. A GPU is not needed to check it, so it is
    /// checked in the plain suite that runs everywhere rather than only in the
    /// e2e run that needs two backends.
    #[test]
    fn a_padded_readback_compacts_back_to_a_tight_image() {
        // Three rows of two RGBA pixels each, staged at a 16-byte pitch.
        let staged = vec![
            1, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            9, 10, 11, 12, 13, 14, 15, 16, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, //
            17, 18, 19, 20, 21, 22, 23, 24, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA,
        ];
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(
            packed,
            (1u8..=24).collect::<Vec<u8>>(),
            "the padding bytes must not survive"
        );
    }

    /// The unpadded case is the one every existing caller was already on, and
    /// it must stay a straight copy.
    #[test]
    fn an_unpadded_readback_is_copied_verbatim() {
        let staged: Vec<u8> = (0u8..32).collect();
        assert_eq!(compact_rows(&staged, 8, 8, 32), staged);
        // A staging buffer larger than the image is truncated, never read past.
        assert_eq!(compact_rows(&staged, 8, 8, 16), staged[..16].to_vec());
    }

    /// A backend that returned a short buffer must produce a short image, not
    /// an out-of-range slice.
    #[test]
    fn a_truncated_readback_does_not_panic() {
        let staged: Vec<u8> = (0u8..20).collect();
        let packed = compact_rows(&staged, 16, 8, 24);
        assert_eq!(packed, vec![0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19]);
    }

    /// The pitch rule the wgpu backend enforces, stated as arithmetic: every
    /// width has to land on a multiple of 256 bytes.
    #[test]
    fn the_staged_pitch_is_always_a_legal_wgpu_row_pitch() {
        for width in [1u32, 3, 32, 63, 64, 97, 256, 1920, 4096] {
            let packed = u64::from(width) * 4;
            let staged = packed
                .checked_next_multiple_of(u64::from(READBACK_ROW_ALIGNMENT))
                .expect("no overflow at these widths");
            assert!(staged >= packed, "{width}: padding may not shrink a row");
            assert_eq!(
                staged % u64::from(READBACK_ROW_ALIGNMENT),
                0,
                "{width}: wgpu refuses this pitch",
            );
            assert_eq!(staged % 4, 0, "{width}: texel count must be exact");
        }
    }

    /// `--size 4000000000x4000000000` used to reach an unchecked
    /// `width * height * 4`, and `--size 100000x100000` a 40 GB allocation.
    /// Both are now refused, and refused *before* a GPU is opened, which is
    /// what makes this testable without one.
    #[test]
    fn an_absurd_frame_is_refused_before_a_backend_is_opened() {
        for (width, height) in [
            (u32::MAX, u32::MAX),
            (4_000_000_000, 4_000_000_000),
            (100_000, 100_000),
            (MAX_DIMENSION + 1, 1),
            (1, MAX_DIMENSION + 1),
        ] {
            let error = OffscreenSetup::open(width, height, Scene::default())
                .err()
                .unwrap_or_else(|| panic!("{width}x{height} is not a frame"));
            assert!(
                matches!(error, OffscreenError::TooLarge { .. }),
                "{width}x{height}: {error}"
            );
        }
    }

    /// A zero edge would make a swapchain nothing can present, and the error
    /// says so rather than being a validation-layer message later.
    #[test]
    fn a_zero_sized_frame_is_refused() {
        for (width, height) in [(0, 16), (16, 0), (0, 0)] {
            assert!(
                matches!(
                    OffscreenSetup::open(width, height, Scene::default()),
                    Err(OffscreenError::Unusable(_))
                ),
                "{width}x{height} should be unusable"
            );
        }
    }

    /// The sprite scene is only diagnostic if it **batches**, and batching is
    /// the submission order: `A A B A` is three batches, `A A B B` is two and
    /// `A A A A` is one. One batch is exactly the case that hid the
    /// `SV_InstanceID` divergence, because with a single draw the SPIR-V and
    /// WGSL lowerings of the instance index are the same number.
    #[test]
    fn the_sprite_scene_submits_three_batches_and_returns_to_the_first_sheet() {
        let batches: Vec<usize> = SPRITE_ORDER.iter().fold(Vec::new(), |mut runs, sheet| {
            if runs.last() != Some(sheet) {
                runs.push(*sheet);
            }
            runs
        });
        assert_eq!(
            batches,
            vec![0, 1, 0],
            "the scene must interleave its sheets, not group them"
        );
        // The last batch is what the bug got wrong: its instances start at 3,
        // and a backend reading the wrong index draws instance 0 instead.
        assert!(
            SPRITE_ORDER.len() > batches.len(),
            "at least one batch must carry more than one instance"
        );
    }

    /// Every rectangle has to be inside the frame at every size the harness
    /// renders, or the scene silently loses the batch the size cropped — and a
    /// missing batch is the very thing it is there to catch.
    #[test]
    fn every_sprite_rectangle_is_on_screen_at_the_harness_sizes() {
        for (width, height) in [(256u32, 192u32), (97, 61), (1920, 1080), (192, 192)] {
            let half_width = SPRITE_HALF_HEIGHT * (width as f32 / height as f32);
            for rect in SPRITE_RECTS {
                let (left, right) = (rect[0], rect[0] + rect[2]);
                let (bottom, top) = (rect[1], rect[1] + rect[3]);
                assert!(
                    left > -half_width && right < half_width,
                    "{width}x{height}: {rect:?} runs off the side of a ±{half_width} view"
                );
                assert!(
                    bottom > -SPRITE_HALF_HEIGHT && top < SPRITE_HALF_HEIGHT,
                    "{width}x{height}: {rect:?} runs off the top or bottom"
                );
            }
        }
    }

    /// The UI scene has to exercise all three command kinds — a frame of
    /// rectangles never samples the glyph atlas, so a broken atlas binding
    /// would draw an identical picture on both backends.
    #[test]
    fn the_ui_scene_draws_text_a_rect_and_an_outline_inside_the_frame() {
        use crate::ui::draw_list::DrawCommand;

        for extent in [(256u32, 192u32), (97, 61), (1920, 1080)] {
            let list = ui_draw_list(extent);
            let (mut rects, mut outlines, mut texts) = (0, 0, 0);
            for command in list.commands() {
                match command {
                    DrawCommand::Rect { min, max, .. } => {
                        rects += 1;
                        assert!(min.x >= 0.0 && min.y >= 0.0, "{extent:?}: {command:?}");
                        assert!(
                            max.x <= extent.0 as f32 && max.y <= extent.1 as f32,
                            "{extent:?}: {command:?}"
                        );
                    }
                    DrawCommand::RectOutline { .. } => outlines += 1,
                    // The glyphs' extent is the atlas's business, so only the
                    // anchor is checked here.
                    DrawCommand::Text { pos, .. } => {
                        texts += 1;
                        assert!(
                            pos.x >= 0.0 && pos.y >= 0.0 && pos.y < extent.1 as f32,
                            "{extent:?}: {command:?}"
                        );
                    }
                }
            }
            assert!(
                rects >= 2 && outlines == 1 && texts >= 1,
                "{extent:?}: {rects} rect(s), {outlines} outline(s), {texts} text(s)"
            );
        }
    }

    /// One more frame than the ring is deep, so the last one is drawn into an
    /// image an earlier one already used.
    const LAPS: usize = RING_IMAGES as usize + 1;

    /// Every barrier [`OffscreenSetup::draw_and_readback`] records on the
    /// acquired swapchain image tells the truth, and every image goes back into
    /// the ring in [`ResourceState::Present`].
    ///
    /// # Why this is a state machine and not two `assert_eq!`s
    ///
    /// A barrier is a *claim* about the state its image is already in, and both
    /// halves of a wrong claim are silent here: the frame still renders, the
    /// pixels still compare equal, and nothing above the seam ever reads the
    /// state back. This module shipped with `from: ColorAttachment` on the
    /// pre-copy barrier — the state the *last pass* leaves the target in, not
    /// the state the graph hands it back in, which is
    /// [`ForwardRenderer::present_target`]'s `final_state: Present` — and with
    /// no barrier back at all, and the golden suite passed on every backend. It
    /// took Vulkan's validation layer to say so, and only on the first of the
    /// two.
    ///
    /// So the observable has to be the command stream itself, which is what
    /// `crcbl_hal::null`'s recorder is: replaying it with a tracker is the same
    /// check a driver's validation layer performs, in the plain suite that runs
    /// with no GPU at all.
    ///
    /// # Why three frames
    ///
    /// The ring is [`RING_IMAGES`] deep, so the third acquire is the first that
    /// hands back an image a previous frame already used. A residual state is
    /// invisible until then: it is legal for the graph to declare `Undefined`
    /// coming in — every backend accepts that as "discard the contents" — but
    /// D3D12 spells both `Undefined` and `Present` `D3D12_RESOURCE_STATE_COMMON`
    /// and validates the *declared* before-state, so an image left in
    /// `TransferSrc` makes the next trip's declaration false. `Present` at the
    /// hand-back is what makes it true, and lap three is where the two differ.
    #[test]
    fn every_readback_barrier_declares_the_state_the_image_is_actually_in() {
        use crate::hal::null::{Command, Event, NullInstance, Recorder};

        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let mut setup = OffscreenSetup::open_on(
            Box::new(instance),
            16,
            16,
            Scene::Cube,
            OffscreenSetup::OPTIONAL_FEATURES,
        )
        .expect("the null backend opens an offscreen setup");
        for _ in 0..LAPS {
            setup
                .draw_and_readback()
                .expect("the null backend records a frame and reads it back");
        }
        setup.finish().expect("the null device reaches idle");

        // The state each swapchain image was last left in, by handle. Two
        // entries at most, so a list rather than a map.
        let mut tracked: Vec<(crate::hal::ImageHandle, ResourceState)> = Vec::new();
        // The image each lap acquired, in order — asserted below to repeat.
        let mut per_lap: Vec<crate::hal::ImageHandle> = Vec::new();
        // The lap's own barriers, held until its copy names the image they are
        // about: the pre-copy barrier is recorded before the copy that
        // identifies it.
        let mut pending: Vec<crate::hal::ImageBarrier> = Vec::new();
        let mut current: Option<crate::hal::ImageHandle> = None;

        for event in recorder.events() {
            match event {
                Event::Acquired { .. } => {
                    assert!(
                        current.is_none(),
                        "a lap acquired again before presenting the image it held"
                    );
                    pending.clear();
                }
                Event::Command {
                    command: Command::Barrier { images, .. },
                    ..
                } => pending.extend(images),
                Event::Command {
                    command: Command::CopyImageToBuffer(copy),
                    ..
                } => {
                    assert!(
                        current.is_none(),
                        "this frame copies one image back per lap; a second copy \
                         means the image a barrier is about can no longer be \
                         identified by it"
                    );
                    current = Some(copy.image);
                    per_lap.push(copy.image);
                }
                Event::Presented { .. } => {
                    let image = current
                        .take()
                        .expect("a lap presents the image it copied back");
                    for barrier in pending.drain(..).filter(|it| it.image == image) {
                        let slot = tracked.iter_mut().find(|(handle, _)| *handle == image);
                        let was = slot.as_ref().map_or(ResourceState::Undefined, |(_, s)| *s);
                        // `Undefined` is the one declaration that is true from
                        // anywhere: it discards the contents rather than
                        // claiming to know them.
                        assert!(
                            barrier.from == ResourceState::Undefined || barrier.from == was,
                            "a barrier declared {:?} -> {:?} on a swapchain image that is \
                             in {was:?}",
                            barrier.from,
                            barrier.to,
                        );
                        match slot {
                            Some((_, state)) => *state = barrier.to,
                            None => tracked.push((image, barrier.to)),
                        }
                    }
                    let (_, state) = tracked
                        .iter()
                        .find(|(handle, _)| *handle == image)
                        .expect("the lap barriered the image it copied");
                    assert_eq!(
                        *state,
                        ResourceState::Present,
                        "present takes the image back into the ring, and the next trip \
                         round declares Undefined — which D3D12 spells COMMON and \
                         validates, so anything but Present here is a state the next \
                         lap's declaration contradicts"
                    );
                }
                _ => {}
            }
        }

        assert_eq!(
            per_lap.len(),
            LAPS,
            "every lap must have copied its image back, or the barriers of the \
             ones that did not were never checked"
        );
        assert_eq!(
            per_lap[0],
            per_lap[LAPS - 1],
            "the ring is {RING_IMAGES} deep, so lap {LAPS} must re-use lap 1's image; \
             a ring that handed out a fresh image every time would never re-read a \
             residual state"
        );
        assert_ne!(
            per_lap[0], per_lap[1],
            "consecutive laps must take different ring images"
        );
    }

    /// The frame the requested [`Scene`] promises, and nothing left behind.
    ///
    /// Every scene reaches the same [`OffscreenSetup::draw_and_readback`], and
    /// what tells them apart is only which passes get hung off the swapchain
    /// import — so the passes the device actually executed are the observable
    /// that says the right one ran. A `Sprite` frame that quietly drew a cube,
    /// or a `Ui` frame whose composite pass dropped out because
    /// [`UiRenderer::add_pass`](crate::render::UiRenderer::add_pass) found
    /// nothing to draw, both hand back the same pixel count and the same `Ok`.
    ///
    /// This test replaced one that opened the null backend, hand-rolled a
    /// swapchain and a graph beside this module rather than through it, and
    /// asserted nothing at all — its own doc comment conceded it proved the
    /// module compiles. [`crate::backend`] already covers opening the null
    /// backend by name, and [`crate::render`] covers the forward pass list, so
    /// what is left for this module to own is the composition above: which
    /// passes each scene contributes, the shape of the bytes handed back, and
    /// [`OffscreenSetup::finish`] giving back everything the setup took.
    #[test]
    fn every_scene_records_the_passes_it_names_and_gives_back_every_object_at_finish() {
        use crate::hal::null::{Event, NullInstance, Recorder};

        // `(kind, label)` as `Command::opens_pass` reports them.
        //
        // **The cube scene's compute triple appears once per cull, and there is
        // one cull per shadow cascade beside the camera's.** Built rather than
        // written out, so the list tracks
        // `crcbl_render::shadow::CASCADES` instead of being a literal that
        // silently stops matching when the count changes — which is the one
        // thing this assertion exists to notice, since a cascade whose cull
        // never ran draws an empty tile and an entirely lit frame.
        //
        // Topic 18's clustering dispatch joins the camera's triple and no
        // cascade's, because a cascade shades nothing: one froxel grid per
        // camera is the whole of what the light list costs a frame.
        //
        // **A shadowed light adds one more triple and nothing else**, which is
        // the one thing the spot slice changed about a frame's shape: a light
        // that was given an atlas tile gets a cull of its own, exactly as a
        // cascade does, and a light that was not given one costs nothing at all.
        // `forward_passes(0)` and `forward_passes(1)` differing by one triple is
        // what says the free tiles really are free.
        let forward_passes = |shadowed_lights: usize| {
            let mut passes: Vec<(&str, &str)> = Vec::new();
            for cull in 0..=(crcbl_render::shadow::CASCADES + shadowed_lights) {
                passes.extend([
                    ("compute", "clear-counters"),
                    ("compute", "cull"),
                    ("compute", "draw-args"),
                ]);
                if cull == 0 {
                    passes.push(("compute", "light-cluster"));
                }
            }
            // `docs/plan/18-render-features.md`'s occlusion slice added the
            // middle three, in this order and no other: the prepass has to write
            // the depth `ssao` reads, `ssao-blur` has to have raw occlusion to
            // blur, and `forward` has to have the blurred channel before it can
            // scale its ambient by it. A frame that runs them in any other order
            // still draws — each pass reads whatever the last frame left in the
            // pooled transient — so the sequence is asserted here rather than
            // trusted to the graph.
            passes.extend([
                ("render", "shadow"),
                ("render", "depth-prepass"),
                ("render", "ssao"),
                ("render", "ssao-blur"),
                ("render", "forward"),
                ("render", "tonemap"),
            ]);
            passes
        };
        let cube_passes = forward_passes(0);
        // The spot scene's one light is a spot, it is the only candidate, and
        // there are tiles left after the cascades — so it holds one.
        let spot_shadow_passes = forward_passes(1);
        // **One triple, not three**, and that is the point-light budget visible
        // in a frame's shape: `Scene::Lights` has three point lights, each of
        // which is six tiles, and the light region holds six — so the most
        // influential one is shadowed and the other two light without occluding.
        let lights_passes = forward_passes(1);
        let expected: [(Scene, &[(&str, &str)]); 8] = [
            (Scene::Cube, &cube_passes),
            // Not the cube scene's passes: `Scene::Lights` is the cube scene
            // with a longer light list, the clustering dispatch is one per
            // camera however many lights it assigns — and one of those lights
            // is shadowed, which costs the cull triple `lights_passes` carries.
            (Scene::Lights, &lights_passes),
            // The same passes: `Scene::Dunes` is the same renderer with
            // different content, and a scene that quietly stopped running the
            // cull pair would still draw a plausible frame.
            (Scene::Dunes, &cube_passes),
            // **One triple more**, and it is the whole of what a shadowed light
            // costs a frame: `crcbl_render::shadow::Selection` gave this scene's
            // spot a tile, so the shadow pass runs a cull against the light's own
            // frustum before drawing into it. A scene whose light was refused a
            // tile records the cube scene's list unchanged — which is
            // `docs/plan/18-render-features.md`'s "still lights, does not
            // occlude" visible in the frame's shape rather than only in its
            // pixels.
            (Scene::SpotShadow, &spot_shadow_passes),
            // **One triple, not six.** A point light's six faces are six
            // viewports and six matrices over *one* cull — topic 18's fourth
            // decision — so a shadowed point light costs a frame exactly what a
            // shadowed spot does. A frame that culled per face would record five
            // more triples here.
            (Scene::PointShadow, &spot_shadow_passes),
            // The cube scene's passes again: `Scene::Ao` is a different room
            // under a different sun, and neither of those is a pass. Its sun is
            // directional and its light list is empty, so it holds no atlas tile
            // — which is what makes this row `cube_passes` and not
            // `spot_shadow_passes`.
            (Scene::Ao, &cube_passes),
            (
                Scene::Sprite,
                &[("render", "scene background"), ("render", "sprites")],
            ),
            (
                Scene::Ui,
                &[("render", "scene background"), ("render", "ui-composite")],
            ),
        ];

        for (scene, passes) in expected {
            let recorder = Recorder::new();
            let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
            let before = recorder.total_live_objects();

            let mut setup = OffscreenSetup::open_on(
                Box::new(instance),
                16,
                16,
                scene,
                OffscreenSetup::OPTIONAL_FEATURES,
            )
            .expect("the null backend opens an offscreen setup");
            recorder.clear(); // the setup's uploads are not this frame's passes

            let (extent, pixels) = setup
                .draw_and_readback()
                .expect("the null backend records a frame and reads it back");
            assert_eq!(extent, (16, 16), "{scene:?}");
            assert_eq!(
                pixels.len(),
                16 * 16 * 4,
                "{scene:?}: the caller gets tightly packed RGBA, padding dropped"
            );

            let recorded: Vec<(String, String)> = recorder
                .events()
                .into_iter()
                .filter_map(|event| match event {
                    Event::Command { command, .. } => command.opens_pass().map(|(kind, label)| {
                        (
                            kind.to_string(),
                            label
                                .expect("every pass this module adds is labelled")
                                .to_string(),
                        )
                    }),
                    _ => None,
                })
                .collect();
            let expected: Vec<(String, String)> = passes
                .iter()
                .map(|(kind, label)| ((*kind).to_string(), (*label).to_string()))
                .collect();
            assert_eq!(recorded, expected, "{scene:?}");
            // And the bound every sample sizes its timers with really does
            // cover them. Asserted here rather than beside the list above
            // because these are the passes the *device* was handed, which is
            // the same stream `PassTimers` brackets — a bound short of it times
            // a prefix of the frame and drops the rest, which is what the
            // samples' hand-picked `8` had been doing.
            assert!(
                recorded.len() <= crcbl_render::MAX_TIMED_PASSES as usize,
                "{scene:?}: {} passes recorded, past the bound of {}",
                recorded.len(),
                crcbl_render::MAX_TIMED_PASSES
            );

            setup.finish().expect("the null device reaches idle");
            assert_eq!(
                recorder.total_live_objects(),
                before,
                "{scene:?}: finish must give back every object the setup and the frame took"
            );
            recorder.assert_valid();
        }
    }
}
