//! GPU setup for options: the shared [`crcbl::engine`] join, and two passes.
//!
//! Everything that is not this sample's — opening a backend, choosing an adapter
//! that can present, the swapchain, the frames-in-flight ring, resize and
//! teardown — lives in [`crcbl::engine::GpuContext`], which every other sample's
//! `gpu.rs` uses too.
//!
//! # There is no scene, and that is the sample
//!
//! A settings screen is measured in pixels against the surface, which is what
//! the UI pass has always drawn in, so there is nothing here for a
//! camera to project and nothing for a sprite to be —
//! `docs/plan/sample/20-options.md` claims sample rule 11's exemption on the
//! same ground `apps/hud` does. What is behind the panel is one clear colour.
//!
//! # The frame is [`crcbl::engine::PageBundle`]'s
//!
//! `backdrop` (clear) → `ui` (the panel, then the debug overlay over it). The UI
//! loads the target rather than clearing it, so declaring it before the clear
//! would wipe the panel away.
//!
//! That order is the bundle's rather than this file's: four samples wrote it
//! out and it is one piece of knowledge — the build order, the acquire →
//! begin-frame → graph → compile → present. What is left here is this sample's name, its
//! clear colour, and the forwards the engine's macros resolve against.

use crcbl::engine::{FrameOutcome, GpuContext, GpuContextDesc, GpuError, GpuOptions, PageBundle};
use crcbl::ui::draw_list::DrawList;
use crcbl::ui::menu::MenuSkin;
use crcbl::ui::text::FontAtlas;

/// What the screen is drawn over: a flat, dark ground that leaves the panel the
/// brightest thing in the frame.
pub const BACKDROP: [f32; 4] = [0.04, 0.05, 0.07, 1.0];

/// This sample's device, its swapchain and the renderer it draws with.
///
/// A newtype over [`PageBundle`], which is the whole of it: the UI renderer
/// and the frame's acquire → begin-frame → graph → compile → present. What stays here is this sample's name, its clear colour, and the
/// forwards `crcbl::impl_game_gpu!` resolves against — see that macro for why
/// they are inherent methods rather than a blanket impl.
#[derive(Debug)]
pub struct Gpu(PageBundle);

/// What both [`Gpu::open`] and [`Gpu::request_open`] ask the engine for.
///
/// One value rather than two copies, for the reason every sample gives: the two
/// bring-up paths must open the *same* device, or a feature only one of them
/// requested is a bug nobody sees until the other path runs.
fn desc(gpu: GpuOptions) -> GpuContextDesc<'static> {
    GpuContextDesc {
        label: crate::APP_NAME,
        // The engine's whole bundle, asserted below. A subset spelled out here
        // is a copy, and a copy goes stale the moment the default gains a flag.
        ..GpuContextDesc::from(gpu)
    }
}

// `PendingGpu`, its `poll`, and the blocking and polled `open`s — both routed
// through `desc` above, so the two bring-up paths ask for the same device.
crcbl::impl_polled_bundle!(gpu: Gpu, pending: PendingGpu, desc: desc);

impl Gpu {
    /// Builds this sample's renderers on an already-open context.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the UI compositor refused the device.
    fn from_context(ctx: GpuContext) -> Result<Self, GpuError> {
        let bundle = PageBundle::new(ctx, crate::APP_NAME, BACKDROP)?;
        Ok(Self(bundle))
    }

    /// The extent the swapchain is currently configured at.
    #[must_use]
    pub const fn extent(&self) -> (u32, u32) {
        self.0.extent()
    }

    /// The engine's context, for the run-level knobs that are not this sample's.
    ///
    /// `crcbl::impl_game_gpu!` forwards
    /// [`HoldsContext`](crcbl::engine::HoldsContext) to this, and
    /// [`arm_screenshot`](crcbl::engine::arm_screenshot) is what reaches it.
    pub const fn context_mut(&mut self) -> &mut GpuContext {
        self.0.context_mut()
    }

    /// See [`PageBundle::take_draw_list`].
    pub fn take_draw_list(&mut self, dl: &mut DrawList) {
        self.0.take_draw_list(dl);
    }

    /// See [`PageBundle::menu_skin`].
    #[must_use]
    pub const fn menu_skin(&self) -> &MenuSkin {
        self.0.menu_skin()
    }

    /// See [`PageBundle::timings`].
    #[must_use]
    pub fn timings(&self) -> Option<&crcbl::render::FrameTimings> {
        self.0.timings()
    }

    /// See [`PageBundle::counters`].
    #[must_use]
    pub fn counters(&self) -> crcbl::render::FrameCounters {
        self.0.counters()
    }

    /// See [`PageBundle::video`].
    #[must_use]
    pub const fn video(&self) -> &crcbl::settings::VideoSettings {
        self.0.video()
    }

    /// See [`PageBundle::atlas`].
    #[must_use]
    pub const fn atlas(&self) -> &FontAtlas {
        self.0.atlas()
    }

    /// Records, submits and presents one frame.
    ///
    /// # Errors
    ///
    /// [`GpuError`] for anything except a swapchain that has merely gone out of
    /// date, which is reported as [`FrameOutcome::Reconfigured`].
    pub fn frame(&mut self) -> Result<FrameOutcome, GpuError> {
        self.0.frame()
    }

    /// Resizes the swapchain.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if the reconfigure failed.
    pub fn resize(&mut self, extent: (u32, u32)) -> Result<(), GpuError> {
        self.0.resize(extent)
    }

    /// Releases everything, in dependency order.
    ///
    /// # Errors
    ///
    /// [`GpuError`] if waiting for outstanding work failed.
    pub fn destroy(self) -> Result<(), GpuError> {
        self.0.destroy()
    }
}

// ---------------------------------------------------------------------------
// The engine's seams
// ---------------------------------------------------------------------------

// The forwards `crcbl::engine` calls this bundle through. Every one of them is a
// method above; the macro is what stops a sample forgetting one.
crcbl::impl_game_gpu!(Gpu);

// Start-up, driven by `crcbl::engine::PolledBoot` rather than blocked on.
crcbl::impl_polled_gpu!(gpu: Gpu, pending: PendingGpu);

#[cfg(test)]
mod tests {
    use super::*;

    /// **The bundle this sample opens with is the engine's own.** A hand-written
    /// list is a copy, and a copy goes stale the moment
    /// [`GpuContextDesc::default`] gains a flag — silently, because the missing
    /// capability changes no picture, only whether the engine's pacing loop and
    /// display-timing query can reach anything.
    #[test]
    fn the_features_this_sample_asks_for_are_the_engine_s_own() {
        let asked = desc(GpuOptions::default());
        assert_eq!(asked.label, crate::APP_NAME);
        assert_eq!(
            asked.optional_features,
            GpuContextDesc::default().optional_features,
            "a subset spelled out here is a copy, and a copy goes stale",
        );
    }
}
