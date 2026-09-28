//! The renderer half of a [`GameGpu`](crate::engine::GameGpu) forward: the
//! bodies every bundle's `apply_video` and `set_debug_view` call, and the
//! process-global video convars they move.

use crcbl_console::{Kind, Value};
use crcbl_render::{
    r_ssao_bent_normals, r_ssao_blur_passes, r_ssao_slices, shadow::r_shadow_filter,
};

use super::engine_video::{
    ANISOTROPIC_FILTERING_KEY, SSAO_BLUR_PASSES_KEY, SSAO_BLUR_PASSES_KIND, SSAO_SLICES_KEY,
    SSAO_SLICES_KIND, VIDEO_NAMESPACE, VideoSettings, ssao_blur_passes_default,
    ssao_slices_default,
};
use super::stage::Unsupported;

#[cfg(doc)]
use super::stage::Stage;

#[cfg(test)]
static PROCESS_VIDEO_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Serialises tests that move process-global video convars and restores them.
#[cfg(test)]
pub(crate) struct ProcessVideoTestGuard {
    prior: [Value; 4],
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
pub(crate) fn process_video_test_guard() -> ProcessVideoTestGuard {
    let lock = PROCESS_VIDEO_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ProcessVideoTestGuard {
        prior: [
            r_shadow_filter.get(),
            r_ssao_slices.get(),
            r_ssao_blur_passes.get(),
            r_ssao_bent_normals.get(),
        ],
        _lock: lock,
    }
}

#[cfg(test)]
impl Drop for ProcessVideoTestGuard {
    fn drop(&mut self) {
        for (convar, value) in [
            (&r_shadow_filter, &self.prior[0]),
            (&r_ssao_slices, &self.prior[1]),
            (&r_ssao_blur_passes, &self.prior[2]),
            (&r_ssao_bent_normals, &self.prior[3]),
        ] {
            convar
                .set(value)
                .expect("restore the test's process-global video convar");
        }
    }
}

/// Keep a manually constructed [`VideoSettings`] value inside an SSAO convar's domain.
fn applicable_ssao_integer(key: &str, kind: Kind, value: i64, default: i64) -> i64 {
    let dotted = format!("{VIDEO_NAMESPACE}.{key}");
    if kind.check(&dotted, &Value::Int(value)).is_ok() {
        value
    } else {
        crcbl_core::log::warn!(
            "settings: `{dotted}` cannot be applied as {value}; the frame uses the shipped value"
        );
        default
    }
}

/// Put the process-global video knobs into force.
///
/// Called when a context reads its startup settings and when a live write hands
/// the whole section to a renderer. `r_shadow_filter` is process-global, so it
/// has to move before the renderer records its first frame.
pub(crate) fn apply_process_video(video: &VideoSettings) {
    r_shadow_filter
        .set(&Value::Enum(video.shadow_filter.label()))
        .expect("`VideoSettings` holds a `Filter`, so its label is in `r_shadow_filter`'s domain");
    let slices = applicable_ssao_integer(
        SSAO_SLICES_KEY,
        SSAO_SLICES_KIND,
        video.ssao_slices,
        ssao_slices_default(),
    );
    r_ssao_slices
        .set(&Value::Int(slices))
        .expect("the settings and `r_ssao_slices` domains match");
    let blur_passes = applicable_ssao_integer(
        SSAO_BLUR_PASSES_KEY,
        SSAO_BLUR_PASSES_KIND,
        video.ssao_blur_passes,
        ssao_blur_passes_default(),
    );
    r_ssao_blur_passes
        .set(&Value::Int(blur_passes))
        .expect("the settings and `r_ssao_blur_passes` domains match");
    r_ssao_bent_normals
        .set(&Value::Bool(video.ssao_bent_normals))
        .expect("a bool is in `r_ssao_bent_normals`' domain");
}

/// Put `video` into force on `renderer`, through the `device` that built it.
///
/// **The body every bundle's
/// [`GameGpu::apply_video`](crate::engine::GameGpu::apply_video) forwards to.**
/// Every bundle in `apps/` that holds a
/// [`ForwardRenderer`](crcbl_render::ForwardRenderer) has to do exactly this,
/// and a copy each is a chance each to forget the effect request or to hand the
/// scale to the anisotropy; there is one copy here and a line each there.
///
/// It writes all four of the renderer's player-facing knobs rather than the one
/// that moved, because [`VideoSettings`] is the section and a caller holding one
/// does not know which key produced it. Setting a knob to the value it already
/// holds costs nothing —
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)
/// returns early on a bit-equal ask, and the other two are field writes.
///
/// The frame ceiling in `video` is deliberately **not** applied: it is the
/// loop's, not the renderer's, and [`Stage::set_frame_limit`] is where it goes.
///
/// # Errors
///
/// [`Unsupported`] where the device refused the sampler the anisotropy asked
/// for. The renderer is left holding the sampler it had — that is
/// [`ForwardRenderer::set_anisotropy`](crcbl_render::ForwardRenderer::set_anisotropy)'s
/// own guarantee — and the failure is **also** logged, naming the key, on this
/// module's "a line that does nothing
/// says so" terms: `Unsupported` says the value did not reach the frame, and the
/// log line is where what the device said survives.
pub fn apply_video_to(
    renderer: &mut crcbl_render::ForwardRenderer,
    device: &dyn crcbl_hal::Device,
    video: &VideoSettings,
) -> Result<(), Unsupported> {
    renderer.set_render_scale(video.render_scale);
    apply_process_video(video);
    let mut request = renderer.effect_request();
    request.video = video.effects;
    request.antialiasing = video.antialiasing;
    renderer.set_effect_request(request);
    renderer
        .set_anisotropy(device, video.anisotropic_filtering)
        .map_err(|error| {
            crcbl_core::log::warn!(
                "settings: `{VIDEO_NAMESPACE}.{ANISOTROPIC_FILTERING_KEY}` did not reach the \
                 frame: {error}; the page is still sampled at {}",
                renderer.anisotropy()
            );
            Unsupported
        })
}

/// Which of [`ForwardRenderer`](crcbl_render::ForwardRenderer)'s debug
/// switches `view` turns on.
///
/// In
/// [`ForwardRenderer::debug_view`](crcbl_render::ForwardRenderer::debug_view)'s
/// own precedence order — bent normal, motion, occlusion, heatmap, LOD, normals,
/// shadow atlas, cascades — so `debug_view` of a renderer this was
/// applied to answers back the view it was handed, whichever it was. That
/// round trip is what
/// `every_debug_view_sets_exactly_the_switch_its_precedence_reads_back` asserts
/// without a device.
///
/// A `match` on the whole enum rather than a row of comparisons, so a
/// [`DebugView`](crcbl_render::DebugView) variant added later fails to compile
/// here instead of silently drawing the shaded frame.
#[must_use]
pub const fn debug_view_switches(view: crcbl_render::DebugView) -> [bool; 8] {
    use crcbl_render::DebugView as V;
    match view {
        V::Shaded => [false, false, false, false, false, false, false, false],
        V::BentNormal => [true, false, false, false, false, false, false, false],
        V::Motion => [false, true, false, false, false, false, false, false],
        V::AmbientOcclusion => [false, false, true, false, false, false, false, false],
        V::Heatmap => [false, false, false, true, false, false, false, false],
        V::LodTint => [false, false, false, false, true, false, false, false],
        V::Normals => [false, false, false, false, false, true, false, false],
        V::ShadowAtlas => [false, false, false, false, false, false, true, false],
        V::Cascades => [false, false, false, false, false, false, false, true],
    }
}

/// Draw `view` on `renderer` instead of the shaded picture.
///
/// **The body every bundle's
/// [`GameGpu::set_debug_view`](crate::engine::GameGpu::set_debug_view) forwards
/// to**, on [`apply_video_to`]'s terms. It writes **every** switch rather than
/// the one the view names, because they are independent and
/// [`ForwardRenderer::debug_view`](crcbl_render::ForwardRenderer::debug_view)
/// resolves them by precedence: leaving an outer one standing would draw a view
/// nobody asked for. [`debug_view_switches`] is the table.
pub const fn set_debug_view_on(
    renderer: &mut crcbl_render::ForwardRenderer,
    view: crcbl_render::DebugView,
) {
    let [
        bent,
        motion,
        occlusion,
        heatmap,
        lod,
        normals,
        atlas,
        cascades,
    ] = debug_view_switches(view);
    renderer.set_bent_normal_view(bent);
    renderer.set_motion_view(motion);
    renderer.set_occlusion_view(occlusion);
    renderer.set_heatmap(heatmap);
    renderer.set_lod_view(lod);
    renderer.set_normals_view(normals);
    renderer.set_atlas_view(atlas);
    renderer.set_cascade_view(cascades);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Process application falls back rather than panicking on invalid public fields.
    #[test]
    fn process_application_rejects_invalid_ssao_counts_without_panicking() {
        let _process_video = process_video_test_guard();
        let video = VideoSettings {
            ssao_slices: i64::MAX,
            ssao_blur_passes: i64::MIN,
            ..VideoSettings::unrestricted()
        };

        apply_process_video(&video);

        assert_eq!(r_ssao_slices.get_i64(), ssao_slices_default());
        assert_eq!(r_ssao_blur_passes.get_i64(), ssao_blur_passes_default());
    }

    /// **Every debug view turns on exactly the switch
    /// [`crcbl_render::ForwardRenderer::debug_view`] reads it back off**, and
    /// leaves every other switch alone.
    ///
    /// The renderer needs a device and these do not, so this asserts the table
    /// against that function's precedence order directly: bent normal, motion,
    /// occlusion, heatmap, LOD, normals, shadow atlas, cascades. A view that set
    /// two switches would be drawn as whichever is outermost, silently — and the
    /// two at the bottom are the ones that would go unnoticed, because each
    /// loses to everything above it.
    #[test]
    fn every_debug_view_sets_exactly_the_switch_its_precedence_reads_back() {
        use crcbl_render::DebugView as V;
        let order = [
            V::BentNormal,
            V::Motion,
            V::AmbientOcclusion,
            V::Heatmap,
            V::LodTint,
            V::Normals,
            V::ShadowAtlas,
            V::Cascades,
        ];
        assert_eq!(
            debug_view_switches(V::Shaded),
            [false; 8],
            "the shaded frame is every switch off",
        );
        for (index, view) in order.into_iter().enumerate() {
            let switches = debug_view_switches(view);
            assert_eq!(
                switches.iter().filter(|on| **on).count(),
                1,
                "{view:?} sets more than one switch",
            );
            assert_eq!(
                switches.len(),
                order.len(),
                "the table has a switch no view names, or one it does not"
            );
            assert!(
                switches[index],
                "{view:?} is not at its precedence position"
            );
        }
    }

    /// A live renderer application moves the process-global SSAO bundle.
    #[test]
    fn applying_video_to_a_renderer_updates_ssao_convars() {
        use crate::hal::null::NullInstance;
        use crate::hal::{DeviceDesc, Format, Instance as _, QueueKind};

        let _process_video = process_video_test_guard();
        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            crcbl_render::ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("the null backend accepts every descriptor");
        let video = VideoSettings {
            anisotropic_filtering: 1.0,
            ssao_slices: 2,
            ssao_blur_passes: 1,
            ssao_bent_normals: false,
            ..VideoSettings::unrestricted()
        };

        apply_video_to(&mut renderer, device.as_ref(), &video)
            .expect("the null backend accepts disabled anisotropy");

        assert_eq!(
            (
                r_ssao_slices.get_i64(),
                r_ssao_blur_passes.get_i64(),
                r_ssao_bent_normals.get_bool(),
            ),
            (2, 1, false),
        );
        renderer.destroy(device.as_ref());
    }

    /// **Every view `set_debug_view_on` is handed reaches the renderer**, which
    /// answers back with that same view.
    ///
    /// The check above is about [`debug_view_switches`] alone, and a table is
    /// only half of this: the other half is the row of setter calls under it,
    /// where a literal in place of a destructured flag, or two rows crossed,
    /// leaves the table right and the frame wrong. Sabotaging
    /// `renderer.set_atlas_view(atlas)` to `set_atlas_view(false)` left the
    /// check above green, which is what this one exists for — it fails on that
    /// edit, and on the same edit to any of the other seven.
    ///
    /// The null backend, so it runs on every CI leg: what is being observed is
    /// a switch moving and
    /// [`ForwardRenderer::debug_view`](crcbl_render::ForwardRenderer::debug_view)
    /// resolving it, and neither reads a driver.
    #[test]
    fn every_debug_view_set_on_a_renderer_reads_back_as_itself() {
        use crate::hal::null::NullInstance;
        use crate::hal::{DeviceDesc, Format, Instance as _, QueueKind};
        use crcbl_render::DebugView as V;

        let instance = NullInstance::gpu_driven();
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .expect("the null backend always opens");
        let queue = device.queue(QueueKind::Graphics).expect("always present");
        let mut renderer =
            crcbl_render::ForwardRenderer::new(device.as_ref(), queue, Format::Rgba8UnormSrgb)
                .expect("the null backend accepts every descriptor");

        for view in [
            V::Shaded,
            V::BentNormal,
            V::Motion,
            V::AmbientOcclusion,
            V::Heatmap,
            V::LodTint,
            V::Normals,
            V::ShadowAtlas,
            V::Cascades,
        ] {
            set_debug_view_on(&mut renderer, view);
            assert_eq!(
                renderer.debug_view(),
                view,
                "{view:?} was applied to a renderer that then drew {:?}",
                renderer.debug_view()
            );
        }

        renderer.destroy(device.as_ref());
    }
}
