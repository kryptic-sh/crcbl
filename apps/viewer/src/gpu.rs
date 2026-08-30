//! The device, the swapchain and the three passes this milestone draws.
//!
//! Everything that is not this application's — opening a backend, choosing an
//! adapter that can present, the swapchain, the frames-in-flight ring, resize
//! and teardown — lives in [`crcbl::engine::GpuContext`], the same join every
//! sample's `gpu.rs` uses.
//!
//! # The document, the menu, the grid and the panel
//!
//! `docs/plan/sample/05-viewer.md`'s milestone 1 is "load + orbit + grid", and
//! all three are here. The grid floor is
//! [`crcbl::render::grid`]'s screen-space pass, switched on through
//! [`ForwardRenderer::set_ground_grid`] — **not** a mesh in the scene. That
//! matters for this sample in particular: the
//! [`SceneDesc`](crcbl::render::scene::SceneDesc) is sized by the glTF
//! conversion for the user's document alone, so a grid made of geometry would
//! need a resident mesh and a material row that the document did not ask for,
//! and its lines would change width with the zoom the turntable is at.
//!
//! It is drawn after the tonemap and depth-tested against the scene, so it sits
//! *under* the model and keeps the same colour at any exposure — which is what
//! makes it a reference rather than part of the picture.
//!
//! The menu and UI passes are here because this sample is hosted by
//! [`crcbl::engine::Loop`] now and the loop draws through them — see
//! [`crate::app`]. Two things go through the UI pass: rule 4's debug panel,
//! which is the engine's, and [`crate::listing`]'s panel, which is the
//! viewer's own and the only thing it draws on top of the user's document.
//! There is still no HUD.
//!
//! # The key light rides with the camera
//!
//! Every other sample's sun is a fixture of its world, because its world is
//! authored and lit on purpose. This one has no world: it has whatever file the
//! user opened, and a sun fixed in world space would leave a model in silhouette
//! from half the angles a turntable reaches — which for a tool whose whole job
//! is showing an asset is the failure, not a style. So the light is built from
//! the camera each frame, over the viewer's shoulder and to one side, which is
//! the three-quarter key every DCC viewport opens with. See [`key_light`].

use crcbl::engine::{
    FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions, PendingGpuContext,
};
use crcbl::hal::{CommandEncoderDesc, Device, Features};
use crcbl::math::{Mat4, Vec3};
use crcbl::prelude::*;
use crcbl::render::{
    MAX_TIMED_PASSES, MenuRenderer, PassTimers, RenderEffects, SkinRange, SkinnedInstanceDesc,
    SkinnedMesh, Skinning, SkinningDesc, SkinningError, TransientPool, UiRenderer, grid::GridStyle,
    scene::InstanceDesc,
};
use crcbl::shaders::skinning::SkinBinding;
use crcbl::shell::{Shell, WindowId};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::{Menu, MenuLayout};
use crcbl::ui::text::FontAtlas;

/// How many frames the swapchain keeps in flight, which is what the pass timers
/// have to be sized for.
const FRAMES_IN_FLIGHT: usize = crcbl::engine::FRAMES_IN_FLIGHT;

/// How far behind the eye the key light sits, relative to how far above and
/// aside — see [`key_light`]. The three together are a direction, so only their
/// ratio matters.
const KEY_BEHIND: f32 = 1.0;
/// How far above.
const KEY_ABOVE: f32 = 0.7;
/// How far to the viewer's left.
const KEY_ASIDE: f32 = 0.5;

/// How bright the key light is.
///
/// Above 1.0 like every other light in this engine: the scene target is
/// `Rgba16Float` and the tonemap is what brings it back, so a key that peaked at
/// 1.0 would be one this pipeline could not tell from a dimmer one.
const KEY_INTENSITY: f32 = 2.0;

/// The flat ambient term, which is the only indirect light there is.
///
/// Larger than `apps/lantern`'s, and deliberately: that fixture is lit to show
/// what its lighting does, and this one has to keep the side of a model that
/// faces away from the key readable, because a user turning a model round is
/// looking at that side on purpose.
const AMBIENT: f32 = 0.25;

/// This application's device, its swapchain and its one renderer.
#[derive(Debug)]
pub struct Gpu {
    ctx: GpuContext,
    renderer: ForwardRenderer,
    pool: TransientPool,
    /// This frame's camera, written by [`Gpu::set_camera`] before the frame is
    /// recorded. The light is derived from it — see [`key_light`] — so there is
    /// no second field that could disagree with it.
    camera: Camera,
    /// UI compositing, for the debug overlay and the listing panel.
    ///
    /// The viewer has no HUD and is not getting one — what it draws on top of
    /// the user's document is asked for and off by default. It has a UI pass
    /// because `docs/plan/sample/00-samples-overview.md` rule 4 applies to it
    /// too, and while this application owned its loop it could not honour that
    /// at all: `--debug-overlay` parsed and reached nothing. See [`crate::app`].
    ui: UiRenderer,
    /// The menu pass: its own sheets, its own screen-space camera, and a pass
    /// that declares nothing on a frame with no menu on it.
    menu: MenuRenderer,
    atlas: FontAtlas,
    draw_list: DrawList,
    /// The pass that deforms this document's skinned geometry, and the regions
    /// it writes — `None` and empty for a document nothing skins, which is most
    /// of them.
    ///
    /// The two are one thing and are built and released together: a [`Skinning`]
    /// with no region to write is a pipeline nothing dispatches, and a region
    /// with no pass to fill it is a draw of whatever the vertex pool was
    /// holding. See [`Gpu::build_skinning`].
    skinning: Option<Skinning>,
    /// One reservation per [`crate::model::Skinned::instances`] entry, in that
    /// order — see [`SkinnedDraw`].
    skinned: Vec<SkinnedDraw>,
    /// This frame's joint palette, written by [`Gpu::set_palette`] before the
    /// frame is recorded.
    ///
    /// One palette for every skinned region, because this application poses one
    /// skeleton: `crate::model::skinned_of` places an instance here only when
    /// its node wears the skin `crate::anim::playable_of` converted, so every
    /// region in the list above is deformed by the same joints.
    ///
    /// Refilled in place rather than replaced, so a frame costs no allocation —
    /// the same bargain `crate::anim::Player` keeps with `crcbl-anim`.
    palette: Vec<Mat4>,
    /// `None` on a device without timestamp queries — the debug panel's GPU
    /// section degrades, the frame does not.
    timers: Option<PassTimers>,
    /// Whether this adapter has [`Features::POLYGON_MODE_LINE`], read once at
    /// open.
    ///
    /// Asked before anything is switched on so the answer can be *reported* —
    /// see [`Gpu::set_wireframe`]. A key that quietly did nothing on a browser,
    /// which has no line fill mode at all, is the failure this field exists to
    /// prevent.
    wireframe_supported: bool,
    /// Whether the graph dump has been logged since the graph last changed
    /// shape. Once per shape rather than once per frame, because a dump every
    /// frame is a log nobody reads.
    dumped: bool,
    /// The last frame's graph dump, kept only for this crate's own tests: it is
    /// how a test sees whether the UI pass was in the frame at all. `add_pass`
    /// declares nothing when the draw list is empty, so the pass's presence in
    /// this string *is* "the overlay reached the GPU".
    #[cfg(test)]
    last_dump: String,
}

/// What both this application's bring-up and any future polled one ask for.
///
/// One value rather than two copies, for the reason every sample gives: two
/// paths that open different devices differ in a way nobody sees until the
/// other one runs.
///
/// # The one feature this sample asks for beyond the engine's own
///
/// [`Features::POLYGON_MODE_LINE`] — the wireframe view's, and the engine's
/// default set has no reason to carry it because no other sample has a
/// wireframe. Added to whatever the engine asks for rather than spelled out
/// beside it, so this list cannot go stale the way a hand-written copy does;
/// `the_features_this_sample_asks_for_are_the_engines_own_plus_the_wireframes`
/// is what keeps that true. It is **optional**: an adapter without it opens
/// exactly as before and the viewer reports that the view is unavailable rather
/// than refusing to start.
/// One skinned instance as the frame holds it: the region the dispatch fills
/// and the bindings that say how.
///
/// The instance itself is not here. [`ForwardRenderer::add_skinned_instance`]
/// keeps a list of its own and re-points every entry of it at the half of its
/// region each frame's dispatch writes, so a handle kept here would be one
/// nothing ever used — this application never moves a skinned object after
/// placing it.
#[derive(Debug)]
struct SkinnedDraw {
    /// The region and its two mesh-table entries, from
    /// [`ForwardRenderer::reserve_skinned`].
    mesh: SkinnedMesh,
    /// One per vertex of the bind-pose run, in the same order —
    /// [`crate::model::SkinnedInstance::bindings`], owned because the pass
    /// uploads them every frame and the document is long gone.
    bindings: Vec<SkinBinding>,
}

/// Flattens a skinning error into the seam's, the way `crcbl-render` flattens
/// its own pool errors.
///
/// This application's constructors and its frame both return
/// [`HalError`]-shaped results and neither has anything to do differently for a
/// palette that was refused than for a buffer that was; the message renders
/// verbatim through [`HalError::Backend`], so the joint or the capacity the
/// pass named survives into the log.
fn hal_error(error: SkinningError) -> HalError {
    match error {
        SkinningError::Hal(hal) => hal,
        other => HalError::Backend(other.to_string()),
    }
}

/// Which of a document's `count` instances a skin deforms, by index.
///
/// A row that is deformed is placed through the skinning seam and must **not**
/// also be placed as an ordinary instance — see the loop in
/// [`Gpu::from_context`], which is the only reason this exists. A mask rather
/// than a scan per row, so a rig on a document with many instances costs one
/// pass rather than one per pair.
///
/// # Panics
///
/// If an entry names a row past `count`, which is one half of a
/// [`crate::model::Model`] disagreeing with the other — see
/// [`Gpu::build_skinning`].
fn skinned_rows(count: usize, skinned: &crate::model::Skinned) -> Vec<bool> {
    let mut mask = vec![false; count];
    for entry in &skinned.instances {
        mask[entry.instance] = true;
    }
    mask
}

/// Gives every reservation in `draws` back to `renderer`.
///
/// The unwind for a half-built [`Gpu::build_skinning`], and only that: a
/// teardown drops them instead, because the pool they are runs of is about to be
/// destroyed whole.
fn release_skinned(renderer: &mut ForwardRenderer, draws: Vec<SkinnedDraw>) {
    for draw in draws {
        renderer.release_skinned(draw.mesh);
    }
}

fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    let base = GpuContextDesc::from(gpu);
    GpuContextDesc {
        label: "viewer",
        optional_features: base.optional_features | Features::POLYGON_MODE_LINE,
        ..base
    }
}

impl Gpu {
    /// Opens a device on `window` and makes `model` resident.
    ///
    /// Takes the whole document rather than its description and its instances,
    /// because a skinned one is three things that have to agree — the scene, the
    /// instances, and which of those instances a skin deforms — and
    /// [`grid_extent_for`] is a fourth derived from a fifth. This is the shape
    /// [`PendingGpu`] already carried, so both bring-up paths now name one
    /// argument and cannot be handed a rig from a different file.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend opened, or if the renderer refused the
    /// description — which for a converted glTF means a document larger than
    /// the pools it asked to be sized for.
    pub fn open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        model: &crate::model::Model,
    ) -> Result<Self, GpuError> {
        Self::from_context(
            GpuContext::open(shell, window, extent, &desc(gpu))?,
            &model.render.scene,
            &model.render.instances,
            &model.skinned,
            grid_extent_for(model),
        )
    }

    /// Builds the renderer and the two UI passes on an already-open context.
    ///
    /// Split from [`Gpu::open`] the way every other sample splits one out: the
    /// context is where the player's `[engine.video]` settings are read, so a
    /// test that wants to say what they are has to be able to hand one over.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the renderer refused the description, the grid or an
    /// instance, or if any HAL call fails.
    fn from_context(
        ctx: GpuContext,
        scene: &crcbl::render::scene::SceneDesc<'_>,
        instances: &[InstanceDesc],
        skinned: &crate::model::Skinned,
        grid_extent: f32,
    ) -> Result<Self, GpuError> {
        let format = ctx.format();
        let mut renderer =
            match ForwardRenderer::with_scene(ctx.device(), ctx.queue(), format, scene) {
                Ok(renderer) => renderer,
                Err(error) => {
                    ctx.destroy()?;
                    return Err(GpuError::Hal(error));
                }
            };
        // The player's `[engine.video]` clamp, which the context read while it
        // opened. It only ever removes, so a run with no settings file draws
        // what it drew before. `reload` carries the answer across a document
        // change, beside the exposure and the wireframe.
        renderer.set_effect_request(ctx.effect_request());
        // `[engine.video] render_scale`, from the same read. `1.0` for a player
        // who has said nothing, which is the extent the surface already had and
        // no upscale pass at all, so a run with no settings file draws what it
        // drew before this line existed.
        renderer.set_render_scale(ctx.render_scale());
        // `docs/plan/sample/05-viewer.md` milestone 1's grid floor — see the
        // [module docs](self) for why it is a pass rather than a mesh.
        //
        // Scaled to the document rather than [`GridStyle::default`]'s metre
        // cell. glTF is authored in metres, but nothing makes an *asset* human
        // sized: a five-centimetre part sits inside one default cell and a
        // five-hundred-metre one is entirely past the default fade, and this is
        // the application whose whole job is opening a file nobody curated.
        //
        // Unwound by hand for the reason the loop below is.
        if let Err(error) =
            renderer.set_ground_grid(ctx.device(), Some(GridStyle::for_extent(grid_extent)))
        {
            renderer.destroy(ctx.device());
            ctx.destroy()?;
            return Err(GpuError::Hal(error));
        }
        // `[engine.video] anisotropic_filtering`, from the same read as the
        // scale: the engine's default for a player who has said nothing, which
        // is the sampler `with_scene` already built, so the call creates
        // nothing. Before the first frame, so that frame's slot rebuilds its
        // groups at its own `begin_frame` rather than a frame late. Unwound by
        // hand for the grid's reason.
        if let Err(error) = renderer.set_anisotropy(ctx.device(), ctx.anisotropic_filtering()) {
            renderer.destroy(ctx.device());
            ctx.destroy()?;
            return Err(GpuError::Hal(error));
        }

        // Unwound by hand, because `Gpu` has no `Drop`: a `?` here would leak
        // every pipeline and buffer the renderer just made.
        //
        // **The rows a skin deforms are deliberately not placed here.**
        // `crate::model::SkinnedInstance` replaces its row rather than joining
        // it, and placing both would draw the mesh twice in one frame — once
        // deformed by the joints and once undeformed at the node's own
        // transform, which is the picture `crate::demo_model`'s `crate.far`
        // would turn into if this loop ignored the distinction.
        let deformed = skinned_rows(instances.len(), skinned);
        for (index, instance) in instances.iter().enumerate() {
            if deformed[index] {
                continue;
            }
            if let Err(error) = renderer.add_instance(instance) {
                renderer.destroy(ctx.device());
                ctx.destroy()?;
                // The seam has no vocabulary for "this pool is full", so
                // `crcbl-render` flattens one into a `HalError` that renders
                // verbatim and keeps the numbers.
                return Err(GpuError::Hal(error.into()));
            }
        }

        let (skinning, skinned_draws) =
            match Self::build_skinning(ctx.device(), &mut renderer, instances, skinned) {
                Ok(built) => built,
                Err(error) => {
                    renderer.destroy(ctx.device());
                    ctx.destroy()?;
                    return Err(GpuError::Hal(error));
                }
            };

        // Unwound by hand for the reason the instance loop above is: `Gpu` has
        // no `Drop`, so a `?` here would leak every pipeline and buffer the
        // renderers before it have made.
        //
        // The skinning pass joins the unwind from here on: it owns buffers and a
        // pipeline of its own, which `ForwardRenderer::destroy` knows nothing
        // about.
        let ui = match UiRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(ui) => ui,
            Err(error) => {
                if let Some(skinning) = skinning {
                    skinning.destroy(ctx.device());
                }
                renderer.destroy(ctx.device());
                ctx.destroy()?;
                return Err(GpuError::Hal(error));
            }
        };
        let menu = match MenuRenderer::new(ctx.device(), ctx.queue(), format) {
            Ok(menu) => menu,
            Err(error) => {
                ui.destroy(ctx.device());
                if let Some(skinning) = skinning {
                    skinning.destroy(ctx.device());
                }
                renderer.destroy(ctx.device());
                ctx.destroy()?;
                return Err(GpuError::Hal(error));
            }
        };
        let timers = PassTimers::new(ctx.device(), FRAMES_IN_FLIGHT, MAX_TIMED_PASSES);
        if timers.is_none() {
            crcbl::log::info!("hal: no timestamp queries on this device; per-pass timing is off");
        }
        // Reported at start-up rather than on the first press, on the timers'
        // terms above: a person who cannot get a wireframe out of this build
        // should be told why by the log they already have, not by a key that
        // does nothing.
        let wireframe_supported = ForwardRenderer::supports_wireframe(ctx.device());
        if !wireframe_supported {
            crcbl::log::info!(
                "viewer: no line fill mode on this device; the wireframe view is unavailable"
            );
        }

        Ok(Self {
            ctx,
            renderer,
            pool: TransientPool::new(),
            camera: Camera::default(),
            ui,
            menu,
            atlas: FontAtlas::built_in(),
            draw_list: DrawList::new(),
            skinning,
            skinned: skinned_draws,
            // Sized for the palette the first frame will bring, so the refill in
            // `set_palette` never grows it.
            palette: Vec::with_capacity(skinned.joints as usize),
            timers,
            wireframe_supported,
            dumped: false,
            #[cfg(test)]
            last_dump: String::new(),
        })
    }

    /// Reserves a region for every instance a skin deforms, places each one, and
    /// builds the pass that fills them.
    ///
    /// `(None, empty)` for a document with nothing to skin, which is the frame
    /// this application drew before skinning existed —
    /// [`Skinning::new`] refuses a pass with no range anyway, and a pass with
    /// nothing to dispatch would cost the vertex pool two barriers a frame for
    /// no reason.
    ///
    /// # Errors
    ///
    /// [`HalError`] for a pool with no room for both halves of a region, a mesh
    /// table with no room for both entries, an instance pool that is full, or a
    /// pass that could not be built. **Every reservation this took is given back
    /// before it returns**, so a refused document leaves the renderer exactly as
    /// it found it and the caller's own unwind has only the renderer to release.
    ///
    /// # Panics
    ///
    /// If a [`crate::model::SkinnedInstance`] names a row past the end of
    /// `instances`. The two come from one [`crate::model::Model`], built by one
    /// walk over one document, so a mismatch is this application disagreeing
    /// with itself rather than anything a file can cause.
    fn build_skinning(
        device: &dyn Device,
        renderer: &mut ForwardRenderer,
        instances: &[InstanceDesc],
        skinned: &crate::model::Skinned,
    ) -> Result<(Option<Skinning>, Vec<SkinnedDraw>), HalError> {
        if skinned.instances.is_empty() || skinned.joints == 0 {
            return Ok((None, Vec::new()));
        }

        let mut draws: Vec<SkinnedDraw> = Vec::with_capacity(skinned.instances.len());
        for entry in &skinned.instances {
            let row = &instances[entry.instance];
            // Reserved and pushed before anything else can fail, so every path
            // out of this loop gives it back — see `release_skinned` below.
            //
            // The row's mesh is a `Geometry::Flat`, which is what
            // `reserve_skinned` panics without: `crcbl_scene::gltf_render`
            // builds no DAGs, so no document this application can open has a
            // level for a dispatch to miss.
            match renderer.reserve_skinned(row.mesh) {
                Ok(mesh) => draws.push(SkinnedDraw {
                    mesh,
                    bindings: entry.bindings.clone(),
                }),
                Err(error) => {
                    release_skinned(renderer, draws);
                    return Err(error.into());
                }
            }
            let placed = renderer.add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &draws.last().expect("the reservation was just pushed").mesh,
                material: row.material,
                transform: entry.transform,
            });
            if let Err(error) = placed {
                release_skinned(renderer, draws);
                return Err(error.into());
            }
        }

        let ranges = u32::try_from(draws.len()).unwrap_or(u32::MAX);
        let bindings = draws.iter().map(|draw| draw.bindings.len()).sum::<usize>();
        let built = Skinning::new(
            device,
            &SkinningDesc {
                label: Some("viewer skinning"),
                frames: FRAMES_IN_FLIGHT,
                ranges,
                // Every range is posed from the same skeleton — see
                // [`Gpu::palette`] — so the frame's whole palette is one
                // skeleton's joints once per range.
                joints: skinned.joints.saturating_mul(ranges),
                bindings: u32::try_from(bindings).unwrap_or(u32::MAX),
                // **This pool and no other.** A pass built against a different
                // buffer writes vertices no draw of this renderer can reach.
                vertices: renderer.vertex_buffer(),
                // And the boundary between that pool's two streams, which the
                // dispatch writes both of.
                attribute_base: renderer.attribute_base(),
            },
        );
        match built {
            Ok(skinning) => Ok((Some(skinning), draws)),
            Err(error) => {
                release_skinned(renderer, draws);
                Err(hal_error(error))
            }
        }
    }

    /// Replaces the resident scene with a freshly converted document.
    ///
    /// `docs/plan/sample/05-viewer.md` V-F4's GPU half: an artist re-exports and
    /// the frame becomes the new file, with no window reopened and no device
    /// lost.
    ///
    /// # The live renderer is only released once the new one exists
    ///
    /// A [`ForwardRenderer`] is built *for* a scene — its pools are sized by the
    /// description — so a swap is a new renderer rather than an edit to this
    /// one. Which means an error has somewhere useful to go: the whole of the
    /// new one is built, filled and checked before the old one is touched, so a
    /// document that is too large for its pools, or that was caught mid-write
    /// and parsed into nonsense, leaves the viewer drawing exactly what it was
    /// drawing. Nothing here can leave the caller with no renderer at all.
    ///
    /// The cost is that both are resident for the length of this call. That is
    /// the same peak a second viewer would need, on a machine already holding
    /// the document twice while it converts.
    ///
    /// # What carries over, and what the caller still owes
    ///
    /// The exposure and the wireframe are the renderer's own state, so they are
    /// re-applied here — a re-export that reset the picture's brightness would
    /// be a reload the artist has to undo. The **debug view** is re-applied for
    /// a sharper version of the same reason: it is `crcbl::debug_view`'s now
    /// rather than this sample's, and the loop writes it only where it moved, so
    /// nothing else would ever put it back. The camera needs nothing —
    /// `crate::app::Viewer`'s `draw` writes it every frame.
    ///
    /// **The camera is deliberately not re-framed.** An artist who has just
    /// placed the view to look at one corner of a model does not want it moved
    /// because they saved; `F` re-frames when they do.
    ///
    /// # Errors
    ///
    /// [`HalError`] if the renderer refused the description, the grid or an
    /// instance. The live scene is unchanged in every one of those cases.
    pub fn reload(
        &mut self,
        scene: &crcbl::render::scene::SceneDesc<'_>,
        instances: &[InstanceDesc],
        skinned: &crate::model::Skinned,
        grid_extent: f32,
    ) -> Result<(), HalError> {
        let device = self.ctx.device();
        let mut next =
            ForwardRenderer::with_scene(device, self.ctx.queue(), self.ctx.format(), scene)?;

        // Unwound by hand from here down, for `Gpu::open`'s reason: a `?` would
        // drop the half-built renderer without releasing a pipeline of it.
        if let Err(error) = next.set_ground_grid(device, Some(GridStyle::for_extent(grid_extent))) {
            next.destroy(device);
            return Err(error);
        }
        // Carried for the render scale's reason below: a reload must not
        // quietly sample at the engine's default again for a player who asked
        // for less — or for more. Here rather than beside it because this one
        // can fail, and here the unwind is still one call.
        if let Err(error) = next.set_anisotropy(device, self.renderer.anisotropy()) {
            next.destroy(device);
            return Err(error);
        }
        // The deformed rows are left out here for `Gpu::from_context`'s reason,
        // and through the same mask so the two cannot come to disagree about
        // what "deformed" means.
        let deformed = skinned_rows(instances.len(), skinned);
        for (index, instance) in instances.iter().enumerate() {
            if deformed[index] {
                continue;
            }
            if let Err(error) = next.add_instance(instance) {
                next.destroy(device);
                return Err(error.into());
            }
        }
        let (skinning, draws) = match Self::build_skinning(device, &mut next, instances, skinned) {
            Ok(built) => built,
            Err(error) => {
                next.destroy(device);
                return Err(error);
            }
        };

        next.set_exposure(self.renderer.exposure());
        // The three requested layers, not the resolved set: a reload must not
        // quietly restore an effect the player's settings took away, and the
        // renderer being replaced is the only thing that knows what they were.
        next.set_effect_request(self.renderer.effect_request());
        // Carried for the same reason the request above is: a reload must not
        // quietly draw at full size again for a player who asked for less.
        next.set_render_scale(self.renderer.render_scale());
        // Carried for the same reason again, and it is **not** the caller's
        // state: `crcbl::engine::Loop` hands the debug view to
        // `GameGpu::set_debug_view` only where the value *moved*, so a reload
        // under `debug_view normals` would put a shaded frame back and leave the
        // console, the HUD row and the picture disagreeing until the next press.
        crcbl::settings::set_debug_view_on(&mut next, crcbl::debug_view::current());
        // Through the renderer rather than through `Gpu::set_wireframe`, which
        // logs: a device that refused the view once has already said so, and a
        // line per re-export is a log nobody reads. The answer is what the
        // caller was already holding, so nothing here can disagree with it.
        if self.renderer.wireframe()
            && let Err(error) = next.set_wireframe(device, true)
        {
            crcbl::log::warn!("viewer: the wireframe view did not survive the reload: {error}");
        }

        let previous = core::mem::replace(&mut self.renderer, next);
        previous.destroy(device);
        // **The pass goes with the renderer it was built against**, because it
        // holds that renderer's vertex-pool handle: a `Skinning` kept across a
        // reload would write into a buffer the destroy above has released. The
        // regions go the same way — they are runs of the old pool — so they
        // are dropped rather than released, on `release_skinned`'s terms.
        if let Some(previous) = core::mem::replace(&mut self.skinning, skinning) {
            previous.destroy(device);
        }
        self.skinned = draws;
        self.palette.clear();
        // The graph's shape is a function of the scene, so the dump the log
        // already carries is about a document that is no longer on screen.
        self.dumped = false;
        Ok(())
    }

    /// Draws the document's triangles as lines instead of filling them.
    ///
    /// **Returns the state actually in force**, which is what the caller must
    /// keep: `false` however often it is asked on a device with no line fill
    /// mode, and `false` if the pipeline would not build. A caller that stored
    /// what it *asked* for would show a wireframe in the debug panel while the
    /// frame was solid, which is the silence
    /// [`ForwardRenderer::set_wireframe`] refuses at the seam and this must not
    /// re-introduce a level up.
    ///
    /// Meant to be called on the key's edge and not once a frame: a refusal logs
    /// a line, and a line a frame is a log nobody reads.
    pub fn set_wireframe(&mut self, on: bool) -> bool {
        if let Err(error) = self.renderer.set_wireframe(self.ctx.device(), on) {
            crcbl::log::warn!("viewer: the wireframe view is not available here: {error}");
        }
        self.renderer.wireframe()
    }

    /// Whether this device can draw the wireframe view at all — see
    /// [`Gpu::set_wireframe`].
    #[must_use]
    pub const fn wireframe_supported(&self) -> bool {
        self.wireframe_supported
    }

    /// Which debug channel the frame is drawing, if any.
    ///
    /// Read back off [`ForwardRenderer::debug_view`], which resolves five
    /// independent switches by precedence, rather than kept beside them: what
    /// [`NORMALS_KEY`](crate::app::NORMALS_KEY) and the console's
    /// `debug_view normals` both write is `crcbl::debug_view`, and this is what
    /// the frame actually came out as.
    #[must_use]
    pub const fn debug_view(&self) -> crcbl::render::DebugView {
        self.renderer.debug_view()
    }

    /// Multiplies the tonemap's exposure by `factor` and returns what is now in
    /// force.
    ///
    /// **Multiplicative, because exposure is a scale**: doubling it is one stop
    /// wherever it starts from, where adding a constant is a large change at the
    /// bottom of the range and an imperceptible one at the top.
    ///
    /// [`ForwardRenderer::set_exposure`] clamps, so the answer is what the frame
    /// will actually be drawn with and not what was asked for — the same
    /// distinction [`Gpu::set_wireframe`] makes, and here it is also what stops a
    /// key held against the end of the range from winding up a value it would
    /// take as many presses to unwind.
    pub const fn scale_exposure(&mut self, factor: f32) -> f32 {
        self.renderer
            .set_exposure(self.renderer.exposure() * factor);
        self.renderer.exposure()
    }

    /// Sets the tonemap's exposure outright and returns what is now in force.
    ///
    /// [`Gpu::scale_exposure`]'s sibling, for the control that names a value
    /// rather than a step: a slider handle is a **position in the range**, so a
    /// factor computed from where the exposure happens to be would accumulate
    /// the error of every drag before it. The answer comes back for the same
    /// reason it does there — [`ForwardRenderer::set_exposure`] clamps.
    pub const fn set_exposure(&mut self, exposure: f32) -> f32 {
        self.renderer.set_exposure(exposure);
        self.renderer.exposure()
    }

    /// The exposure the next frame is drawn with.
    #[must_use]
    pub const fn exposure(&self) -> f32 {
        self.renderer.exposure()
    }

    /// Which of topic 18's effects a frame begun now would draw — the
    /// **resolved** set, read back off the renderer.
    ///
    /// Resolved rather than requested, which is why it is asked of the renderer
    /// rather than of the context: the device clamps last and absolutely, so a
    /// request it pared down would otherwise report as granted. [`Gpu::reload`]
    /// carries the request across a document change, so this answers for the
    /// live renderer whichever document is on screen.
    #[must_use]
    pub fn effects(&self) -> RenderEffects {
        self.renderer.resolved_effects()
    }

    /// The extent the swapchain is currently configured at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.ctx.extent()
    }

    /// Where the next frame is drawn from.
    pub const fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
    }

    /// Takes this frame's joint palette — [`crate::anim::Player::palette`].
    ///
    /// **Every skinned region in the frame is deformed by it**, because this
    /// application poses one skeleton: `crate::model::skinned_of` places an
    /// instance in the frame only when its node wears the skin
    /// `crate::anim::playable_of` converted. A frame drawn
    /// without it deforms nothing new: the skinning pass would be handed the
    /// palette of the frame before, or an empty one, and
    /// [`Skinning::begin_frame`] refuses an empty palette by name rather than
    /// letting the shader's index clamp make something up.
    ///
    /// Copied rather than kept by reference because the palette is composed
    /// during the game's draw and consumed when the frame is recorded, which are
    /// two calls with the renderer's own borrows in between.
    pub fn set_palette(&mut self, palette: &[Mat4]) {
        self.palette.clear();
        self.palette.extend_from_slice(palette);
    }

    /// The glyph atlas the UI pass renders text from.
    ///
    /// The menu lays itself out with it and the debug overlay measures its own
    /// panel with it, and both must use the *same* atlas the pass draws with or
    /// the background rect is the wrong size for the text inside it.
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        &self.atlas
    }

    /// Takes this frame's menu, or `None` on a frame that shows none.
    ///
    /// CPU only — the upload happens inside [`Gpu::frame`], at the extent the
    /// swapchain was actually acquired at.
    pub fn set_menu(&mut self, menu: Option<(&Menu, &MenuLayout)>) {
        self.menu.set_menu(menu);
    }

    /// Takes this frame's UI geometry, handing the previous frame's allocation
    /// back so the loop can refill it instead of building a new one.
    pub fn take_draw_list(&mut self, list: &mut DrawList) {
        std::mem::swap(&mut self.draw_list, list);
    }

    /// The most recent frame whose per-pass GPU timings have landed.
    ///
    /// `None` on a device with no timestamp queries, and empty for the first few
    /// frames — the report is deliberately frames latent; see
    /// [`crcbl::render::PassTimers`].
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.timers.as_ref().map(PassTimers::latest)
    }

    /// What the last [`Gpu::frame`] recorded: draws, instances and triangles,
    /// summed over the three renderers this bundle holds.
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.renderer
            .counters()
            .plus(self.menu.counters())
            .plus(self.ui.counters())
    }

    /// The `[engine.video]` section this bundle's context read while opening.
    ///
    /// Forwarded rather than answered, so a run reports the player's file
    /// rather than a default — see [`crcbl::engine::GameGpu::video`].
    #[must_use]
    pub const fn video(&self) -> &crcbl::settings::VideoSettings {
        self.ctx.video()
    }

    /// The UI geometry this frame handed over, for this crate's own tests.
    #[cfg(test)]
    #[must_use]
    pub const fn draw_list(&self) -> &DrawList {
        &self.draw_list
    }

    /// The last frame's render-graph dump, for this crate's own tests.
    #[cfg(test)]
    #[must_use]
    pub fn last_dump(&self) -> &str {
        &self.last_dump
    }

    /// Records, submits and presents one frame.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for anything except a swapchain that has merely gone out of
    /// date, which is reported as [`FrameOutcome::Reconfigured`].
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        let Some(acquired) = self.ctx.acquire()? else {
            self.dumped = false;
            return Ok(FrameOutcome::Reconfigured);
        };
        // The extent the swapchain answered with, not the one that was asked
        // for: they differ on the frame a resize lands.
        let extent = acquired.extent;

        let light = key_light(&self.camera);
        // **The skinned entry point is a different call rather than a flag**,
        // and the seam is written that way on purpose: it rotates the instance
        // ring, moves the ping-pong and re-points every skinned object at the
        // half this frame's dispatch fills, in the one order that is correct. A
        // frame that took the plain path would leave every skinned object
        // pointing at the pose of the frame before, for ever, with nothing to
        // report it.
        match self.skinning.as_mut() {
            Some(skinning) => {
                // One range per reserved region, all four fields of each tied
                // together by `SkinnedMesh::skin_range` so a region and a
                // bind-pose base cannot come from different meshes.
                let ranges: Vec<SkinRange<'_>> = self
                    .skinned
                    .iter()
                    .map(|draw| draw.mesh.skin_range(&self.palette, &draw.bindings))
                    .collect();
                self.renderer
                    .begin_skinned_frame(
                        self.ctx.device(),
                        skinning,
                        &ranges,
                        &self.camera,
                        &light,
                        extent,
                    )
                    .map_err(|error| GpuError::Hal(hal_error(error)))?;
            }
            None => {
                self.renderer
                    .begin_frame(self.ctx.device(), &self.camera, &light, extent)?;
            }
        }
        self.menu
            .begin_frame(self.ctx.device(), extent)
            .map_err(GpuError::Hal)?;
        // Upload this frame's UI geometry: the listing panel and the debug
        // overlay, both of which are off unless asked for.
        self.ui
            .begin_frame(self.ctx.device(), &self.draw_list, &self.atlas, 1.0)
            .map_err(GpuError::Hal)?;

        let format = self.ctx.format();
        let compiled = {
            let mut graph = RenderGraph::new(self.ctx.queue());
            let target = graph.import_image(
                "swapchain",
                ForwardRenderer::present_target(acquired.image, acquired.view, format, extent),
            );
            // **The dispatch is added by the renderer, not beside it.** The
            // skinned entry point takes the pass itself so it can add it first
            // and declare the read on every pass that pulls vertices out of the
            // pool; a caller that imported the pool itself would order nothing,
            // and the draws would read the region before the compute write was
            // visible — a mesh that flickers between two poses on hardware and
            // looks perfect under a validation layer.
            let _hdr = match self.skinning.as_ref() {
                Some(skinning) => self
                    .renderer
                    .add_skinned_passes(&mut graph, &self.pool, target, extent, skinning),
                None => self
                    .renderer
                    .add_passes(&mut graph, &self.pool, target, extent),
            };
            // **Between the scene and the text, and that order is the whole
            // join.** The menu's scrim dims what is already in the target, so it
            // has to come after the tonemap; the panel is opaque and the labels
            // are UI-pass text, so it has to come before the UI or the frame
            // paints over its own words.
            self.menu.add_pass(&mut graph, target);
            // Composited on top of the tonemapped scene, so the overlay is
            // readable over whatever the document drew.
            self.ui.add_pass(&mut graph, target, extent);
            graph.compile(&self.pool)?
        };

        #[cfg(test)]
        {
            self.last_dump = compiled.dump();
        }
        // "The graph must be able to explain itself" — §2.4's debug-tools
        // principle.
        if !self.dumped {
            crcbl::log::debug!("render graph for the viewer frame:\n{}", compiled.dump());
            self.dumped = true;
        }

        let mut encoder = self
            .ctx
            .device()
            .create_command_encoder(&CommandEncoderDesc {
                label: Some("viewer frame"),
                queue: self.ctx.queue(),
            });
        compiled.execute(
            self.ctx.device(),
            &mut self.pool,
            encoder.as_mut(),
            self.timers.as_mut(),
        )?;
        let command_buffer = encoder.finish()?;

        let outcome = self.ctx.submit_and_present(&acquired, command_buffer)?;
        // Only after the submit's retire, so nothing the pool destroys can
        // still be referenced by a submission that has not completed.
        self.pool.retire_unused(self.ctx.device());
        if outcome == FrameOutcome::Reconfigured {
            self.dumped = false;
        }
        Ok(outcome)
    }

    /// Resizes the swapchain.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        self.ctx.resize(extent)?;
        self.dumped = false;
        Ok(())
    }

    /// Releases everything, in dependency order.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed.
    pub fn destroy(mut self) -> Result<(), GpuError> {
        self.ctx.drain()?;
        self.ui.destroy(self.ctx.device());
        self.menu.destroy(self.ctx.device());
        self.pool.destroy(self.ctx.device());
        if let Some(timers) = self.timers.as_mut() {
            timers.destroy(self.ctx.device());
        }
        // Before the renderer, because it borrowed that renderer's vertex-pool
        // handle. The regions themselves are dropped rather than released: they
        // are runs of the pool the line below destroys, so giving them back
        // would be bookkeeping in a free list nothing reads again — see
        // [`release_skinned`].
        if let Some(skinning) = self.skinning.take() {
            skinning.destroy(self.ctx.device());
        }
        self.skinned.clear();
        self.renderer.destroy(self.ctx.device());
        self.ctx.destroy()
    }
}

// The forwards `crcbl::engine` calls this bundle through. Every one of
// them is a method above; the macro is what stops a sample forgetting one.
/// The device request this sample's bundle is waiting on, and the document it
/// will build the renderer out of when it arrives.
///
/// **The document is carried through the wait**, which is the whole reason
/// [`crcbl::engine::PolledGpu::Context`] exists. Every other sample's bundle can
/// be built from the window and the options, so its pending state holds only the
/// context request; this one opens with the glTF it was asked to show, and there
/// is no default document to stand in for it while the device arrives.
///
/// An [`Rc`](std::rc::Rc) rather than the model itself, because
/// [`crate::app::PendingLoop`] needs the same document when it assembles the
/// loop — the camera is framed on its bounds and the panel lists its contents —
/// and the alternative is converting the file twice.
#[derive(Debug)]
pub struct PendingGpu {
    pending: PendingGpuContext,
    model: std::rc::Rc<crate::model::Model>,
}

impl PendingGpu {
    /// Advances the open. `Ok(None)` means "not yet, poll again next frame".
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the device request failed, or if the renderer refused
    /// the document — which for a converted glTF means one larger than the
    /// pools it asked to be sized for.
    pub fn poll(&mut self) -> Result<Option<Gpu>, GpuError> {
        match self.pending.poll()? {
            Some(ctx) => Gpu::from_context(
                ctx,
                &self.model.render.scene,
                &self.model.render.instances,
                &self.model.skinned,
                grid_extent_for(&self.model),
            )
            .map(Some),
            None => Ok(None),
        }
    }
}

/// How wide the ground grid is drawn for `model`.
///
/// The largest axis rather than the diagonal, so a long thin model does not get
/// a cell sized for a span it only has in one direction. Written once because
/// both bring-up paths need the same number and a second spelling is where they
/// would drift.
#[must_use]
pub fn grid_extent_for(model: &crate::model::Model) -> f32 {
    model.bounds.half_extent().max_element() * 2.0
}

impl Gpu {
    /// Asks for a device and returns at once, keeping `model` for the build.
    ///
    /// The non-blocking half of [`Gpu::open`], routed through the same
    /// `desc` so the two paths cannot ask for different devices.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if no backend could be opened at `extent`.
    pub fn request_open<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        model: std::rc::Rc<crate::model::Model>,
    ) -> Result<PendingGpu, GpuError> {
        Ok(PendingGpu {
            pending: GpuContext::request_open(shell, window, extent, &desc(gpu))?,
            model,
        })
    }
}

/// Lets [`crcbl::engine::PolledBoot`] drive this bundle's arrival.
///
/// Written out rather than taken from `crcbl::impl_polled_gpu!` because that
/// macro is for a bundle with no context: it declares `type Context = ()` and
/// calls a four-argument `request_open`. This one opens with a document.
impl crcbl::engine::PolledGpu for Gpu {
    type Pending = PendingGpu;

    /// The document to build the renderer from. See [`PendingGpu`].
    type Context = std::rc::Rc<crate::model::Model>;

    fn request<S: Shell + ?Sized>(
        shell: &S,
        window: WindowId,
        extent: (u32, u32),
        gpu: GpuOptions,
        model: Self::Context,
    ) -> Result<Self::Pending, GpuError> {
        Self::request_open(shell, window, extent, gpu, model)
    }

    fn poll_pending(pending: &mut Self::Pending) -> Result<Option<Self>, GpuError> {
        pending.poll()
    }
}

/// The two seams `crcbl::settings::apply` reaches a renderer through.
///
/// A second inherent block rather than lines inside the one above, so the
/// forward `crcbl::impl_game_gpu!(Gpu, with_renderer)` picks up sits beside the
/// invocation that needs it. `docs/plan/52-debug-console.md` decision 3 is where
/// the pair comes from, and `crcbl::settings` holds both bodies — every bundle
/// with a `ForwardRenderer` writes exactly these two lines.
impl Gpu {
    /// Put the player's `[engine.video]` section into force now.
    ///
    /// # Errors
    ///
    /// `crcbl::settings::apply_video_to`'s: the device refused the sampler the
    /// anisotropy asked for.
    fn apply_video(
        &mut self,
        video: &crcbl::settings::VideoSettings,
    ) -> Result<(), crcbl::settings::Unsupported> {
        crcbl::settings::apply_video_to(&mut self.renderer, self.ctx.device(), video)
    }

    /// Draw `view` instead of the shaded picture.
    ///
    /// # Errors
    ///
    /// None: this bundle has the renderer the view needs.
    fn set_debug_view(
        &mut self,
        view: crcbl::render::DebugView,
    ) -> Result<(), crcbl::settings::Unsupported> {
        crcbl::settings::set_debug_view_on(&mut self.renderer, view);
        Ok(())
    }
}

crcbl::impl_game_gpu!(Gpu, with_renderer);

/// The key light for a frame drawn from `camera`.
///
/// Behind the eye, above it and to its left, on the camera's *own* axes — so
/// the model is lit from the same relative angle however the turntable is
/// turned, and the side the user brought round to look at is the lit one. The
/// basis is built the way [`Camera::view`] builds its own, which is what makes
/// "the camera's right" mean one thing in this file and in the matrix the frame
/// is drawn with.
///
/// [`DirectionalLight::direction`] points *towards* the light, which is why
/// nothing here is negated twice.
#[must_use]
pub fn key_light(camera: &Camera) -> DirectionalLight {
    let forward = (camera.target - camera.eye).normalize_or_zero();
    // Degenerate only if the eye is on the pivot, which `OrbitCamera` clamps
    // against — `normalize_or_zero` is what keeps a `NaN` out of the uniform
    // block if some other caller ever does it anyway.
    let right = forward.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(forward);
    DirectionalLight {
        direction: (-forward * KEY_BEHIND + up * KEY_ABOVE - right * KEY_ASIDE).normalize_or_zero(),
        color: Vec3::splat(KEY_INTENSITY),
        ambient: Vec3::splat(AMBIENT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl::render::{EffectOverride, EffectRequest, OrbitCamera, Projection};
    use crcbl::screenshot::{ForwardScene, OffscreenSetup};

    /// The frame the grid proof below is drawn at.
    ///
    /// Small on purpose — the claim is about which pixels moved, not about how
    /// many — but tall enough that "the top eighth of the frame" is several rows
    /// rather than one.
    const PROOF_EXTENT: (u32, u32) = (160, 120);

    /// One frame of an otherwise **empty** scene, with the ground grid switched
    /// on or off, read back off the GPU.
    ///
    /// Empty because the grid is the subject: with no instance placed, every
    /// pixel that differs between the two frames is one the grid pass wrote, and
    /// nothing has to be assumed about where a mesh landed.
    ///
    /// The camera stands two metres up looking **horizontally**, so the ground
    /// plane fills the lower half of the frame and the horizon is across the
    /// middle. That is what makes the top of the frame a control: a ray leaving
    /// the upper half never reaches `y = 0`, so nothing the grid draws can land
    /// there.
    fn grid_proof_frame(on: bool) -> (u32, u32, Vec<u8>) {
        let mut setup = OffscreenSetup::open_forward(
            PROOF_EXTENT.0,
            PROOF_EXTENT.1,
            move |device, queue, format| {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                if on {
                    renderer.set_ground_grid(device, Some(GridStyle::default()))?;
                }
                Ok(ForwardScene {
                    camera: Camera {
                        eye: Vec3::new(0.0, 2.0, 8.0),
                        target: Vec3::new(0.0, 2.0, 0.0),
                        up: Vec3::Y,
                        projection: Projection::Perspective {
                            fov_y: std::f32::consts::FRAC_PI_3,
                            near: 0.01,
                        },
                    },
                    sun: DirectionalLight::default(),
                    renderer: Box::new(renderer),
                })
            },
        )
        .expect("the pinned backend opens");
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame lands");
        setup.finish().expect("teardown");
        (width, height, pixels)
    }

    /// Whether this run has a driver pinned, which is what
    /// [`grid_proof_frame`] needs and what `CRCBL_GPU=vk cargo test -p viewer`
    /// supplies.
    ///
    /// The null backend reads back nothing a pixel claim could be made about, so
    /// the proof below is skipped without a pin rather than made against zeroes.
    /// CI pins it — see the workflow's "Run the viewer's suite against
    /// lavapipe", which is the run this test exists for.
    fn a_driver_is_pinned() -> bool {
        match std::env::var(crcbl::backend::BACKEND_ENV_VAR) {
            Err(_) => false,
            Ok(name) => match GpuBackend::from_name(&name) {
                None => panic!(
                    "{} names {name:?}, which is not a backend",
                    crcbl::backend::BACKEND_ENV_VAR
                ),
                Some(backend) => backend != GpuBackend::Null,
            },
        }
    }

    /// **The grid reaches pixels.** Two readbacks of the same frame, one with
    /// the grid switched on and one without, and they are not the same picture.
    ///
    /// This is the claim `crate::app`'s dump assertion cannot make: a pass in
    /// the graph that bound a pipeline and drew nothing produces exactly that
    /// dump. Only the bytes can tell the two apart.
    ///
    /// Three assertions, and each rules out a different way of passing for the
    /// wrong reason:
    ///
    /// * the **top eighth** of the frame is byte-identical — what the pass drew
    ///   is a plane the camera's upper rays never reach, rather than something
    ///   sprayed over the whole target;
    /// * a real share of the lower half moved — the pass is not a no-op that
    ///   bound a pipeline and drew nothing;
    /// * and not *all* of the lower half moved — lines, not a flood fill.
    #[test]
    fn the_grid_draws_pixels_the_frame_does_not_have_without_it() {
        if !a_driver_is_pinned() {
            return;
        }
        let (width, height, off) = grid_proof_frame(false);
        let (on_width, on_height, on) = grid_proof_frame(true);
        assert_eq!((width, height), (on_width, on_height));
        assert_eq!(off.len(), on.len());

        let row = width as usize * 4;
        let sky = (height / 8) as usize * row;
        assert_eq!(
            &off[..sky],
            &on[..sky],
            "the grid changed pixels above the horizon, where no ray reaches the ground plane",
        );

        let half = (height / 2) as usize * row;
        let ground_pixels = (off.len() - half) / 4;
        let moved = off[half..]
            .chunks_exact(4)
            .zip(on[half..].chunks_exact(4))
            .filter(|(before, after)| before != after)
            .count();
        assert!(
            moved > ground_pixels / 100,
            "only {moved} of {ground_pixels} ground pixels moved, which is not a grid",
        );
        assert!(
            moved < ground_pixels,
            "every one of {ground_pixels} ground pixels moved, which is a wash rather than lines",
        );
    }

    /// **The light stays over the viewer's shoulder wherever the turntable
    /// goes.** A sun fixed in world space would put half of every orbit in
    /// silhouette, which for this application is the bug — see the [module
    /// docs](self).
    ///
    /// The claim is made against the camera's own forward vector, at several
    /// poses, so it is about the relationship and not about one lucky angle.
    #[test]
    fn the_key_light_follows_the_camera_round_the_model() {
        let orbit = OrbitCamera::new(Vec3::new(1.0, 2.0, -3.0), 5.0, Projection::default());
        let mut seen = Vec::new();
        for (yaw, pitch) in [(0.0f32, 0.0f32), (1.7, 0.3), (-2.6, -0.5), (3.1, 0.9)] {
            let mut turned = orbit;
            turned.orbit(yaw, pitch);
            let camera = turned.camera();
            let light = key_light(&camera);

            let forward = (camera.target - camera.eye).normalize();
            assert!(
                light.direction.dot(-forward) > 0.5,
                "at ({yaw}, {pitch}) the light points {:?}, which is not behind an eye \
                 looking {forward:?}",
                light.direction,
            );
            assert!(
                (light.direction.length() - 1.0).abs() < 1e-5,
                "the direction must be a unit vector, and is {:?}",
                light.direction,
            );
            seen.push(light.direction);
        }

        // Anti-vacuity: a light that ignored the camera entirely would satisfy
        // nothing above for the right reason only because every dot product
        // would be computed against the same vector.
        assert!(
            seen.windows(2).any(|pair| pair[0].distance(pair[1]) > 0.1),
            "the light never moved: {seen:?}",
        );
    }

    /// The bundle asks for the engine's own feature set rather than a subset
    /// spelled out here — the check every other sample carries, since one of
    /// them shipped a hand-written list that went stale — **plus the one feature
    /// this sample has a use for that no other sample does**.
    ///
    /// Written as the engine's set with a bit added rather than as a list, which
    /// is what keeps the anti-staleness property the original assertion had: a
    /// feature the engine starts or stops asking for moves both sides of this.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engines_own_plus_the_wireframes() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, "viewer");
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features | Features::POLYGON_MODE_LINE,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
        assert!(
            !GpuContextDesc::default()
                .optional_features
                .contains(Features::POLYGON_MODE_LINE),
            "the engine started asking for the line fill mode itself, so this sample no longer \
             needs to — fold the addition away rather than leaving two places asking",
        );
    }

    /// One frame of a single cube, filled or as lines, read back off the GPU.
    ///
    /// [`ForwardRenderer::new`]'s demo scene with **one** instance in it, because
    /// the claim below is about which texels a triangle's interior owns: with one
    /// convex solid on screen the covered region is a silhouette, and a wireframe
    /// of it has to be a small fraction of that silhouette rather than all of it.
    ///
    /// The light is this application's own [`key_light`], so the cube is lit from
    /// wherever the camera stands and no face of it can come out equal to the
    /// clear colour by accident.
    /// Takes the antialiasing resolve off a proof frame's renderer.
    ///
    /// **A resolve is a filter over the thing these frames measure.** Every
    /// helper below reads individual texels back and compares them — against a
    /// normal encoding, against the same texel of a second frame, against the
    /// background — and a blend along every silhouette moves exactly those
    /// texels. `crcbl_render::ForwardRenderer::resolved_effects` already takes
    /// the bit off for the debug views that are readouts rather than pictures,
    /// and that argument is this one; these frames are the same kind of thing
    /// and are not covered by it, because two of them read a *shaded* frame as
    /// the comparand.
    ///
    /// Programmatic rather than the camera layer, so it wins over whatever a
    /// settings file asked for — see
    /// [`EffectRequest`](crcbl::render::EffectRequest).
    fn without_the_resolve(renderer: &mut ForwardRenderer) {
        renderer.set_effect_request(EffectRequest {
            programmatic: EffectOverride::none().force(RenderEffects::ANTIALIASING, Some(false)),
            ..EffectRequest::default()
        });
    }

    fn wireframe_proof_frame(on: bool) -> (u32, u32, Vec<u8>) {
        let camera = Camera {
            eye: Vec3::new(2.6, 2.0, 3.4),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::Perspective {
                fov_y: std::f32::consts::FRAC_PI_3,
                near: 0.01,
            },
        };
        let mut setup = OffscreenSetup::open_forward_with(
            PROOF_EXTENT.0,
            PROOF_EXTENT.1,
            // The harness's own list plus the wireframe's, for the reason
            // [`desc`] gives: a device is granted the intersection of what it
            // has and what it was asked for, so a setup that did not ask would
            // refuse the view on hardware that can do it.
            OffscreenSetup::OPTIONAL_FEATURES | Features::POLYGON_MODE_LINE,
            move |device, queue, format| {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer
                    .add_instance(&InstanceDesc {
                        mesh: crcbl::render::scene::DEMO_CUBE,
                        material: crcbl::render::scene::DEMO_UNTINTED,
                        transform: crcbl::math::Mat4::IDENTITY,
                    })
                    .expect("one instance fits in any pool");
                without_the_resolve(&mut renderer);
                if on {
                    renderer.set_wireframe(device, true)?;
                }
                Ok(ForwardScene {
                    camera,
                    sun: key_light(&camera),
                    renderer: Box::new(renderer),
                })
            },
        )
        .expect("the pinned backend opens");
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame lands");
        setup.finish().expect("teardown");
        (width, height, pixels)
    }

    /// One frame of a single cube at `exposure`, read back off the GPU.
    ///
    /// [`wireframe_proof_frame`]'s scene exactly — the same cube, the same
    /// camera, this application's own [`key_light`] — so the only thing that
    /// differs between two calls is the number the tonemap multiplies by.
    ///
    /// `None` never calls the setter at all, which is what makes "the default is
    /// unchanged" a claim about the code path a golden run takes rather than
    /// about a constant.
    fn exposure_proof_frame(exposure: Option<f32>) -> (u32, u32, Vec<u8>) {
        let camera = Camera {
            eye: Vec3::new(2.6, 2.0, 3.4),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::Perspective {
                fov_y: std::f32::consts::FRAC_PI_3,
                near: 0.01,
            },
        };
        let mut setup = OffscreenSetup::open_forward(
            PROOF_EXTENT.0,
            PROOF_EXTENT.1,
            move |device, queue, format| {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer
                    .add_instance(&InstanceDesc {
                        mesh: crcbl::render::scene::DEMO_CUBE,
                        material: crcbl::render::scene::DEMO_UNTINTED,
                        transform: crcbl::math::Mat4::IDENTITY,
                    })
                    .expect("one instance fits in any pool");
                without_the_resolve(&mut renderer);
                if let Some(exposure) = exposure {
                    renderer.set_exposure(exposure);
                }
                Ok(ForwardScene {
                    camera,
                    sun: key_light(&camera),
                    renderer: Box::new(renderer),
                })
            },
        )
        .expect("the pinned backend opens");
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame lands");
        setup.finish().expect("teardown");
        (width, height, pixels)
    }

    /// Linear light from one byte of the swapchain, undoing its sRGB encode.
    ///
    /// IEC 61966-2-1's electro-optical transfer function, which is what an sRGB
    /// swapchain format applies on write — so this is the only way back from a
    /// readback byte to the value the tonemap produced. Written out here as
    /// every other pixel test in this workspace writes it (see
    /// `crcbl/tests/render_e2e.rs`'s `srgb_encode`), and
    /// [`the_transfer_function_pins_its_own_endpoints`] is what says the
    /// transcription is right.
    fn linear_of(byte: u8) -> f32 {
        let encoded = f32::from(byte) / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    }

    /// The decode's fixed points, from the specification rather than from this
    /// implementation: a check computed by the thing it checks is no check.
    #[test]
    fn the_transfer_function_pins_its_own_endpoints() {
        assert!(linear_of(0).abs() < 1e-6);
        assert!((linear_of(255) - 1.0).abs() < 1e-6);
        // The knee: 0.04045 encoded is 0.0031308 linear, and 12.92 is the slope
        // below it.
        assert!((linear_of(10) - (10.0 / 255.0 / 12.92)).abs() < 1e-6);
    }

    /// **The exposure reaches pixels, and it scales the picture the way an
    /// exposure does.**
    ///
    /// `crate::app`'s key test says the presses reach the renderer and
    /// `crcbl_render::forward`'s block test says the number reaches the buffer;
    /// neither can tell a frame drawn with it from a frame that ignored it. Only
    /// the bytes can.
    ///
    /// **Two exposures a factor of four apart, one stop either side of the
    /// default**, and they are chosen to sit where the operator still
    /// discriminates: it is `saturate(colour * exposure)`, so far enough up
    /// every pixel pins to white and two exposures produce the same frame. The
    /// clear behind the cube is about 0.012 to 0.030 in linear light, so even at
    /// 2.0 it is nowhere near the clamp — and `the_bright_frame_is_not_saturated`
    /// below is not a separate test but the third assertion here, which is what
    /// keeps that reasoning honest if the clear colour ever changes.
    ///
    /// Four claims:
    ///
    /// * the corner the cube cannot reach scales by **exactly the exposure
    ///   ratio** in linear light — the claim that this is an exposure and not
    ///   merely something that got brighter;
    /// * no byte anywhere got darker — a brightening, not a different picture;
    /// * a real share got strictly brighter — not a no-op that redrew the same
    ///   frame;
    /// * and the bright frame has unsaturated colour left in it, so the two
    ///   exposures were compared where the operator still tells them apart.
    #[test]
    fn the_exposure_scales_the_frame_the_way_an_exposure_does() {
        if !a_driver_is_pinned() {
            return;
        }
        let (width, height, dark) = exposure_proof_frame(Some(0.5));
        let (bright_width, bright_height, bright) = exposure_proof_frame(Some(2.0));
        assert_eq!((width, height), (bright_width, bright_height));
        assert_eq!(dark.len(), bright.len());

        // The first texel of the frame, which the cube's silhouette cannot
        // reach — `the_wireframe_view_empties_the_interior_and_keeps_the_edges`
        // reads the same one as its background. Byte 3 is alpha, which the
        // tonemap writes as 1.0 and no encode touches, so only the three colour
        // bytes carry the exposure.
        for channel in 0..3 {
            let low = linear_of(dark[channel]);
            let high = linear_of(bright[channel]);
            assert!(low > 0.0, "channel {channel} of the clear is black already");
            let ratio = high / low;
            // 8-bit quantisation at the dark end, and nothing else: the clear is
            // a few hundredths of full scale, where one byte of the encoded
            // value is a couple of percent of the linear one.
            assert!(
                (ratio - 4.0).abs() < 0.4,
                "channel {channel} of the clear went from {low} to {high}, a factor of {ratio} \
                 rather than the 4 the exposures differ by",
            );
        }
        assert_eq!(dark[3], bright[3], "the tonemap writes alpha, not exposure");

        assert!(
            dark.iter().zip(&bright).all(|(low, high)| high >= low),
            "a byte got darker when the exposure went up, so this is a different picture rather \
             than a brighter one",
        );
        let brighter = dark
            .iter()
            .zip(&bright)
            .filter(|(low, high)| high > low)
            .count();
        assert!(
            brighter > dark.len() / 2,
            "only {brighter} of {} bytes moved, which is not a frame that changed exposure",
            dark.len(),
        );
        let unsaturated = bright
            .chunks_exact(4)
            .filter(|texel| texel[..3].iter().any(|byte| *byte < u8::MAX))
            .count();
        assert!(
            unsaturated > bright.len() / 8,
            "only {unsaturated} texels of the bright frame have colour left below white, so 2.0 \
             is past where this operator still discriminates",
        );
    }

    /// **A renderer nobody configures draws the frame it always drew.**
    ///
    /// The check behind "every golden image is unchanged": the default is not
    /// merely equal to the old constant in a doc comment, the frame drawn
    /// without ever calling the setter is byte-identical to the frame drawn with
    /// it set to the default.
    #[test]
    fn the_default_exposure_is_the_frame_drawn_without_one() {
        if !a_driver_is_pinned() {
            return;
        }
        let (width, height, untouched) = exposure_proof_frame(None);
        let (set_width, set_height, set) =
            exposure_proof_frame(Some(crcbl::shaders::tonemap::DEFAULT_EXPOSURE));
        assert_eq!((width, height), (set_width, set_height));
        assert_eq!(
            untouched, set,
            "setting the default has to be the same frame as never setting anything",
        );

        // Anti-vacuity: the same comparison against a different exposure fails,
        // so the equality above is a fact about this frame and not about two
        // readbacks that would match whatever was asked for.
        let (_, _, moved) = exposure_proof_frame(Some(2.0));
        assert_ne!(
            untouched, moved,
            "two different exposures produced the same bytes, so nothing here is measuring the \
             exposure at all",
        );
    }

    /// **The wireframe view draws edges where the filled frame drew a solid.**
    ///
    /// The claim that separates "a second pipeline was bound" from "the picture
    /// is a wireframe", and only the bytes can make it. `crate::app`'s toggle
    /// test says the state reaches the renderer; this says what the rasteriser
    /// then did with it.
    ///
    /// Every assertion is against the **clear colour**, taken from a corner of
    /// the filled frame that the cube's silhouette cannot reach — so "covered"
    /// means "some fragment shaded here" without anything being assumed about
    /// what colour it came out.
    ///
    /// Four claims, and each rules out a different way of passing wrongly:
    ///
    /// * the filled frame covers a real share of the target — otherwise the cube
    ///   missed the camera and every comparison below is between two empty
    ///   frames;
    /// * the wireframe frame covers far less of it — the interior texels became
    ///   background, which is what a wireframe *is* and what a solid frame drawn
    ///   through a differently-labelled pipeline would not do;
    /// * it covers something — lines, not a frame the pipeline swap emptied;
    /// * and nearly every texel it does cover was covered in the filled frame
    ///   too — the edges stayed lit and in place, rather than the geometry having
    ///   moved.
    #[test]
    fn the_wireframe_view_empties_the_interior_and_keeps_the_edges() {
        if !a_driver_is_pinned() {
            return;
        }
        let (width, height, filled) = wireframe_proof_frame(false);
        let (line_width, line_height, lines) = wireframe_proof_frame(true);
        assert_eq!((width, height), (line_width, line_height));
        assert_eq!(filled.len(), lines.len());

        // The clear colour, read off the frame rather than written down: the
        // tonemap and the swapchain's encode both stand between `SCENE_CLEAR`
        // and a byte, and this test is not about either of them.
        let background: [u8; 4] = filled[..4].try_into().expect("a frame has a first pixel");
        assert_eq!(
            &lines[..4],
            &background,
            "the two frames disagree about the corner the cube cannot reach, so `background` is \
             not a background",
        );

        let covered = |frame: &[u8]| {
            frame
                .chunks_exact(4)
                .filter(|texel| *texel != background)
                .count()
        };
        let total = filled.len() / 4;
        let filled_covered = covered(&filled);
        let line_covered = covered(&lines);
        assert!(
            filled_covered > total / 50,
            "only {filled_covered} of {total} texels are the cube, so it is not on screen",
        );
        assert!(
            line_covered * 2 < filled_covered,
            "the wireframe covers {line_covered} texels against the solid's {filled_covered}, \
             which is not an emptied interior",
        );
        assert!(
            line_covered > 0,
            "the wireframe covers nothing at all, which is an emptied frame rather than lines",
        );

        // **A majority, not nearly all**, and the slack is line rasterisation's:
        // an edge on the silhouette is drawn along the boundary of the region the
        // solid filled, so which side of it a texel lands on is the driver's
        // diamond-exit rule and differs between them. A bar that assumed the
        // outer edges landed inside would be a test about one rasteriser. What it
        // still rules out is the geometry having moved, which would leave the two
        // sets nearly disjoint.
        let kept = filled
            .chunks_exact(4)
            .zip(lines.chunks_exact(4))
            .filter(|(solid, line)| *line != background && *solid != background)
            .count();
        assert!(
            kept * 2 > line_covered,
            "only {kept} of the wireframe's {line_covered} covered texels were covered by the \
             solid too, so the lines are not on the model's own edges",
        );
    }

    /// One frame of a single cube seen **square on from `from`**, with the
    /// normals view on or off, read back off the GPU.
    ///
    /// The camera stands on a face's own axis, which is what makes the claim
    /// below a claim about one normal: the cube is back-face culled
    /// (`ForwardRenderer::primitive` names [`CullMode::Back`]) and the other five
    /// faces are edge-on or behind, so every texel the cube covers belongs to the
    /// face pointing at the eye. `up` steps off `+Y` for the two vertical views,
    /// where a look direction along `Y` leaves no basis to build.
    ///
    /// [`CullMode::Back`]: crcbl::hal::CullMode
    fn normals_proof_frame(from: Vec3, on: bool) -> (u32, u32, Vec<u8>) {
        let camera = Camera {
            // Four metres out from a cube one metre across, which frames it with
            // room to spare at the field of view below and leaves the corner this
            // test reads its background from well clear of the silhouette.
            eye: from * 4.0,
            target: Vec3::ZERO,
            up: if from.x == 0.0 && from.z == 0.0 {
                Vec3::Z
            } else {
                Vec3::Y
            },
            projection: Projection::Perspective {
                fov_y: std::f32::consts::FRAC_PI_3,
                near: 0.01,
            },
        };
        let mut setup = OffscreenSetup::open_forward(
            PROOF_EXTENT.0,
            PROOF_EXTENT.1,
            move |device, queue, format| {
                let mut renderer = ForwardRenderer::new(device, queue, format)?;
                renderer
                    .add_instance(&InstanceDesc {
                        mesh: crcbl::render::scene::DEMO_CUBE,
                        material: crcbl::render::scene::DEMO_UNTINTED,
                        transform: crcbl::math::Mat4::IDENTITY,
                    })
                    .expect("one instance fits in any pool");
                without_the_resolve(&mut renderer);
                renderer.set_normals_view(on);
                Ok(ForwardScene {
                    camera,
                    sun: key_light(&camera),
                    renderer: Box::new(renderer),
                })
            },
        )
        .expect("the pinned backend opens");
        let ((width, height), pixels) = setup.draw_and_readback().expect("the frame lands");
        setup.finish().expect("teardown");
        (width, height, pixels)
    }

    /// **The normals view draws each face's world-space normal, encoded
    /// `n * 0.5 + 0.5`.**
    ///
    /// `crate::app`'s key test says the press reaches the renderer and
    /// `crcbl_render::forward`'s block test says the switch reaches the buffer;
    /// neither can tell a frame drawn from the normal from a frame that tinted
    /// everything. Only the bytes can, and only if they are checked against the
    /// **mapping** rather than against "something changed".
    ///
    /// The scene is [`crcbl_shaders::mesh::FACES`]' own cube, looked at square on
    /// down each of the six axes in turn, so the claim is made once per face and
    /// the expected colour comes from the same table the geometry does. Three
    /// things fall out of that which no single frame could say:
    ///
    /// * **+X really is red, +Y green, +Z blue** — a mapping that permuted the
    ///   channels would satisfy any one frame and fail across three;
    /// * **an inverted face reads the complement** — `-X` is `(0, 0.5, 0.5)` where
    ///   `+X` is `(1, 0.5, 0.5)`, which is the whole diagnostic this view exists
    ///   for, and it is checked here rather than asserted in a doc comment;
    /// * **the normals are in world space and not view space** — every one of
    ///   these six frames looks straight down a face's normal, so a view-space
    ///   encoding would paint all six the same colour.
    ///
    /// The bytes are decoded through [`linear_of`] before they are compared,
    /// because the swapchain encodes sRGB on write and the mapping is a statement
    /// about light and not about a byte. The tolerance is a byte's worth of
    /// quantisation at the shallowest part of that curve, with room to spare.
    #[test]
    fn the_normals_view_paints_each_face_the_encoding_of_its_world_normal() {
        if !a_driver_is_pinned() {
            return;
        }
        // One byte either side of 0.5 linear is about 0.006 — see `linear_of`'s
        // curve, which is shallowest in the middle. This is that with room, and
        // still nowhere near half the 0.5 that separates two of the three values
        // an axis-aligned normal can encode to.
        const SLACK: f32 = 0.02;

        for face in &crcbl::shaders::mesh::FACES {
            let normal = Vec3::from_array(face.normal);
            let (width, height, shaded) = normals_proof_frame(normal, false);
            let (normals_width, normals_height, normals) = normals_proof_frame(normal, true);
            assert_eq!((width, height), (normals_width, normals_height));

            // The clear colour, read off the frame rather than written down, on
            // `the_wireframe_view_empties_the_interior_and_keeps_the_edges`'
            // terms: the tonemap and the swapchain's encode both stand between
            // `SCENE_CLEAR` and a byte, and this test is not about either.
            let background: [u8; 4] = shaded[..4].try_into().expect("a frame has a first pixel");
            assert_eq!(
                &normals[..4],
                &background,
                "the {} frames disagree about the corner the cube cannot reach, so `background` \
                 is not a background",
                face.name,
            );

            // What the face's normal encodes to, which is the claim.
            let expected = normal * 0.5 + Vec3::splat(0.5);
            let mut covered = 0usize;
            for (at, (was, now)) in shaded
                .chunks_exact(4)
                .zip(normals.chunks_exact(4))
                .enumerate()
            {
                if was == background {
                    // Nothing was drawn here, so the normals frame must not have
                    // drawn anything either — the geometry did not move.
                    assert_eq!(
                        now, background,
                        "the {} normals frame covers texel {at}, which the shaded frame did not",
                        face.name,
                    );
                    continue;
                }
                covered += 1;
                let read = Vec3::new(linear_of(now[0]), linear_of(now[1]), linear_of(now[2]));
                assert!(
                    (read - expected).abs().max_element() < SLACK,
                    "texel {at} of the {} face reads {read:?}, and its normal {normal:?} encodes \
                     to {expected:?}",
                    face.name,
                );
            }

            // Without this the loop above is vacuous on a frame that missed the
            // cube entirely, and every claim in it would hold over no texels.
            let total = shaded.len() / 4;
            assert!(
                covered > total / 50,
                "only {covered} of {total} texels are the {} face, so it is not on screen",
                face.name,
            );
        }
    }

    /// A settings store holding one `settings.toml`.
    fn settings_file(toml: &str) -> crcbl::store::MemoryStorage {
        use crcbl::store::StorageSource;

        let storage = crcbl::store::MemoryStorage::new();
        storage
            .write(
                std::path::Path::new(crcbl::store::settings::SETTINGS_FILE),
                toml.as_bytes(),
            )
            .expect("memory storage accepts every write");
        storage
    }

    /// The effects one whole start-up resolves to, with `settings` standing in
    /// for the player's file — and the effects the same [`Gpu`] resolves to
    /// after a [`reload`](Gpu::reload) has replaced its renderer.
    ///
    /// The whole path, through [`Gpu::from_context`] rather than around it: a
    /// helper that resolved an [`EffectRequest`](crcbl::render::EffectRequest)
    /// by hand would prove the resolution order works and nothing at all about
    /// whether this application hands the context's request to its renderer.
    ///
    /// Both answers out of one open, because there are two lines to guard and
    /// the second only exists once the first has run: `open`'s
    /// `set_effect_request(ctx.effect_request())` and `reload`'s
    /// `set_effect_request(self.renderer.effect_request())`, which is what
    /// carries the player's clamp onto the renderer built for the new document.
    ///
    /// `scene::demo()` rather than a converted document, because the subject is
    /// the wiring and not the glTF: `Gpu::from_context` treats every scene the
    /// same, and a document read off disk would put a temporary directory
    /// between this test and what it is asking about.
    fn effects_opened_with(
        settings: crcbl::engine::SettingsSource<'_>,
    ) -> (RenderEffects, RenderEffects) {
        use crcbl::engine::{Clock, open_window, wait_for_configure};
        use crcbl::shell::{HeadlessShell, WindowDesc};

        let mut shell = HeadlessShell::new();
        let clock = Clock::new(true);
        let window = open_window(
            &mut shell,
            &clock,
            &WindowDesc {
                title: "viewer",
                app_id: "sh.kryptic.crcbl.viewer",
                ..WindowDesc::default()
            },
        )
        .expect("headless always creates a window");
        let mut events = 0;
        let extent =
            wait_for_configure(&mut shell, window, &mut events).expect("headless configures");

        let ctx = GpuContext::open(
            &shell,
            window,
            extent,
            &GpuContextDesc {
                // The null backend, so this needs no driver and no window
                // system — and the application's own label and feature set, so
                // it is this application's start-up being asked.
                backend: Some(GpuBackend::Null),
                settings,
                ..desc(GpuOptions::default())
            },
        )
        .expect("the null backend opens everywhere");

        // `Skinned::default` is a document with no rig, which is what
        // `scene::demo()` is: this test is about the effect request reaching a
        // renderer, and the skinning seam is not on that path at all.
        let nothing_skinned = crate::model::Skinned::default();
        let mut gpu = Gpu::from_context(
            ctx,
            &crcbl::render::scene::demo(),
            &[],
            &nothing_skinned,
            1.0,
        )
        .expect("the null device builds the viewer's renderer");
        let opened = gpu.effects();
        gpu.reload(&crcbl::render::scene::demo(), &[], &nothing_skinned, 1.0)
            .expect("the null device builds the replacement renderer");
        let reloaded = gpu.effects();
        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
        (opened, reloaded)
    }

    /// **A debug view survives a re-export.**
    ///
    /// The guard for one line in [`Gpu::reload`], and it needs its own check
    /// rather than riding on the effect one because it fails for a reason none
    /// of the other carries has: the debug view is not this bundle's state at
    /// all. It is `crcbl::debug_view`'s, and `crcbl::engine::Loop` writes it to
    /// a bundle only where the value **moved** — so a fresh renderer built
    /// mid-run is the one thing that can silently lose it, and nothing else in
    /// the process would ever put it back.
    ///
    /// Read off [`ForwardRenderer::debug_view`], which resolves the five
    /// switches by precedence, so a carry that set the wrong one fails here.
    #[test]
    fn a_debug_view_survives_a_reload() {
        use crcbl::engine::{Clock, SettingsSource, open_window, wait_for_configure};
        use crcbl::render::DebugView;
        use crcbl::shell::{HeadlessShell, WindowDesc};

        // The view is one process-global value — `crcbl::debug_view` — so this
        // check owns it for its duration and hands it back shaded.
        let _view = crcbl::debug_view::for_test();

        let mut shell = HeadlessShell::new();
        let clock = Clock::new(true);
        let window = open_window(
            &mut shell,
            &clock,
            &WindowDesc {
                title: "viewer",
                app_id: "sh.kryptic.crcbl.viewer",
                ..WindowDesc::default()
            },
        )
        .expect("headless always creates a window");
        let mut events = 0;
        let extent =
            wait_for_configure(&mut shell, window, &mut events).expect("headless configures");
        let ctx = GpuContext::open(
            &shell,
            window,
            extent,
            &GpuContextDesc {
                backend: Some(GpuBackend::Null),
                settings: SettingsSource::None,
                ..desc(GpuOptions::default())
            },
        )
        .expect("the null backend opens everywhere");
        let nothing_skinned = crate::model::Skinned::default();
        let mut gpu = Gpu::from_context(
            ctx,
            &crcbl::render::scene::demo(),
            &[],
            &nothing_skinned,
            1.0,
        )
        .expect("the null device builds the viewer's renderer");

        assert_eq!(gpu.debug_view(), DebugView::Shaded);
        // Through the seam the loop uses, so this is the state a `N` press or a
        // console line would have left behind.
        crcbl::debug_view::set(DebugView::Normals);
        crcbl::settings::set_debug_view_on(&mut gpu.renderer, crcbl::debug_view::current());
        assert_eq!(gpu.debug_view(), DebugView::Normals);

        gpu.reload(&crcbl::render::scene::demo(), &[], &nothing_skinned, 1.0)
            .expect("the null device builds the replacement renderer");
        assert_eq!(
            gpu.debug_view(),
            DebugView::Normals,
            "the re-export put a shaded frame back under a view nothing will re-apply",
        );

        gpu.destroy().expect("teardown");
        shell.destroy_window(window).expect("the window goes away");
    }

    /// **The player's `[engine.video]` clamp reaches the frames this
    /// application draws, and survives a re-export**, which is what the summary
    /// reports.
    ///
    /// The guard for two lines:
    /// `renderer.set_effect_request(ctx.effect_request())` in
    /// [`Gpu::from_context`], and
    /// `next.set_effect_request(self.renderer.effect_request())` in
    /// [`Gpu::reload`]. Deleting either leaves that renderer on
    /// [`EffectRequest::default`](crcbl::render::EffectRequest), whose `video`
    /// layer is [`RenderEffects::all`] — which is *also* what a run with no
    /// settings file resolves to, so the control below cannot catch it and only
    /// a run with a real clamp in front of it can.
    ///
    /// One arm per effect, because a file that switched them all off resolves
    /// to the empty set however few of the keys were wired.
    #[test]
    fn the_players_video_clamp_reaches_the_frame_and_survives_a_reload() {
        use crcbl::engine::SettingsSource;

        let (all_on, all_on_reloaded) = effects_opened_with(SettingsSource::None);
        assert_eq!(
            all_on,
            RenderEffects::DEFAULT_STACK,
            "a run with no settings at all draws the default stack, or the comparisons below \
             are against the wrong control",
        );
        assert_eq!(all_on_reloaded, all_on);

        for (key, off) in [
            ("shadows", RenderEffects::SHADOWS),
            ("ambient_occlusion", RenderEffects::AMBIENT_OCCLUSION),
            ("reflections", RenderEffects::REFLECTIONS),
        ] {
            let storage = settings_file(&format!("[engine.video]\n{key} = false\n"));
            let (opened, reloaded) = effects_opened_with(SettingsSource::Source(&storage));
            let want = RenderEffects::DEFAULT_STACK.difference(off);
            assert_eq!(
                opened, want,
                "`{key} = false` did not reach the renderer this application draws with",
            );
            assert_eq!(
                reloaded, want,
                "`{key} = false` did not survive the re-export — a reload must not quietly \
                 restore an effect the player's settings took away",
            );
        }
    }
}
