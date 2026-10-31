//! That every pass constructor a view builds releases what it made when it
//! fails part-way, counted on the null backend.
//!
//! Each constructor creates its objects one seam call at a time, and most are
//! built lazily between frames — see `View::build_passes` and
//! `View::build_content` — so one that leaked on failure would leak again on
//! every retry. [`Recorder::fail_create_after`] fails each of a constructor's
//! creations in turn; each attempt must return the injected error and leave
//! [`Recorder::total_live_objects`] where it found it. See [`crate::partial`].

use super::*;

/// Frames in flight every constructor here is built for.
const FRAMES: usize = FRAMES_IN_FLIGHT;

/// Fails `build`'s first creation, then its second, and so on until it
/// finishes before reaching the failure, asserting after each failed attempt
/// that it propagated the injected error and left nothing alive. The finished
/// build is destroyed and checked for leaks too, so a constructor that
/// over-releases on success would show up as well.
///
/// Returns how many creations were failed, which is how many `build` makes.
fn fails_cleanly_at_every_creation<T>(
    name: &str,
    recorder: &Recorder,
    device: &dyn Device,
    mut build: impl FnMut(&dyn Device) -> Result<T, HalError>,
    destroy: impl FnOnce(T, &dyn Device),
) -> u32 {
    let mut creates = 0;
    loop {
        let before = recorder.total_live_objects();
        recorder.fail_create_after(creates);
        match build(device) {
            Err(error) => {
                assert!(
                    !recorder.create_failure_pending(),
                    "{name} failed before reaching creation {creates}: {error}"
                );
                assert!(
                    matches!(error, HalError::OutOfDeviceMemory),
                    "{name} did not propagate the injected failure at creation {creates}: {error}"
                );
                assert_eq!(
                    recorder.total_live_objects(),
                    before,
                    "{name} leaked when creation {creates} failed"
                );
            }
            Ok(pass) => {
                assert!(
                    recorder.create_failure_pending(),
                    "{name} succeeded although creation {creates} failed"
                );
                recorder.cancel_create_failure();
                destroy(pass, device);
                assert_eq!(
                    recorder.total_live_objects(),
                    before,
                    "{name} leaked on the success path"
                );
                break;
            }
        }
        creates += 1;
    }
    assert!(
        creates > 0,
        "{name} created nothing, so this checked nothing"
    );
    recorder.assert_valid();
    creates
}

/// A plain sampler, standing in for the renderer's shadow-atlas one.
fn sampler(device: &dyn Device) -> SamplerHandle {
    device
        .create_sampler(&SamplerDesc::default())
        .expect("a default sampler")
}

#[test]
fn the_settings_driven_passes_release_on_every_failure() {
    let (recorder, device, _) = open();
    let device = device.as_ref();
    let fullscreen = ForwardRenderer::build_fullscreen;
    let target = Format::Rgba8UnormSrgb;
    fails_cleanly_at_every_creation(
        "Ssao::new",
        &recorder,
        device,
        |device| Ssao::new(device, FRAMES, fullscreen),
        Ssao::destroy,
    );
    fails_cleanly_at_every_creation(
        "ContactShadows::new",
        &recorder,
        device,
        |device| ContactShadows::new(device, FRAMES, fullscreen),
        ContactShadows::destroy,
    );
    fails_cleanly_at_every_creation(
        "Bloom::new",
        &recorder,
        device,
        |device| Bloom::new(device, FRAMES, fullscreen),
        Bloom::destroy,
    );
    fails_cleanly_at_every_creation(
        "Fxaa::new",
        &recorder,
        device,
        |device| Fxaa::new(device, FRAMES, target, fullscreen),
        Fxaa::destroy,
    );
    fails_cleanly_at_every_creation(
        "Cmaa2::new",
        &recorder,
        device,
        |device| Cmaa2::new(device, FRAMES, target, fullscreen),
        Cmaa2::destroy,
    );
    fails_cleanly_at_every_creation(
        "Upscale::new",
        &recorder,
        device,
        |device| Upscale::new(device, FRAMES, target, fullscreen),
        Upscale::destroy,
    );
    fails_cleanly_at_every_creation(
        "AtlasView::new",
        &recorder,
        device,
        |device| AtlasView::new(device, FRAMES, target, fullscreen),
        AtlasView::destroy,
    );
    // Built by the first frame that draws reflections since the march became
    // its only reader — see `View::build_passes`.
    fails_cleanly_at_every_creation(
        "Hiz::new",
        &recorder,
        device,
        |device| Hiz::new(device, FRAMES, ForwardRenderer::build_depth_fullscreen),
        Hiz::destroy,
    );
}

/// [`Ssao::prepare`] is a constructor of its own — the bent-free filters are
/// built by the first frame that can run them, after [`Ssao::new`] — so it is
/// failed at each of its creations too.
#[test]
fn the_bent_free_filters_release_on_every_failure() {
    let (recorder, device, _) = open();
    let device = device.as_ref();
    let fullscreen = ForwardRenderer::build_fullscreen;
    let alone = fails_cleanly_at_every_creation(
        "Ssao::new",
        &recorder,
        device,
        |device| Ssao::new(device, FRAMES, fullscreen),
        Ssao::destroy,
    );
    let prepared = fails_cleanly_at_every_creation(
        "Ssao::prepare",
        &recorder,
        device,
        |device| {
            let mut ssao = Ssao::new(device, FRAMES, fullscreen)?;
            match ssao.prepare(device, fullscreen, true) {
                Ok(()) => Ok(ssao),
                Err(error) => {
                    ssao.destroy(device);
                    Err(error)
                }
            }
        },
        Ssao::destroy,
    );
    assert!(
        prepared > alone,
        "`Ssao::prepare` created nothing past `Ssao::new`, so none of its failures was checked"
    );
}

#[test]
fn the_content_passes_release_on_every_failure() {
    let (recorder, device, _) = open();
    let device = device.as_ref();
    let shadow_sampler = sampler(device);
    fails_cleanly_at_every_creation(
        "Water::new",
        &recorder,
        device,
        |device| {
            Water::new(
                device,
                FRAMES,
                shadow_sampler,
                ForwardRenderer::build_fullscreen_with,
            )
        },
        Water::destroy,
    );
    // The draw batch's members are created side by side, so this also covers a
    // refusal that leaves batch siblings live.
    fails_cleanly_at_every_creation(
        "Grass::new",
        &recorder,
        device,
        |device| Grass::new(device, FRAMES, shadow_sampler),
        Grass::destroy,
    );
    device.destroy_sampler(shadow_sampler);
    assert_eq!(recorder.total_live_objects(), 0);
}

/// The passes `View::build` makes whole and hands to its own rollback, which
/// only ever held them once they were — so a failure inside one leaked.
#[test]
fn the_view_build_passes_release_on_every_failure() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let fullscreen = ForwardRenderer::build_fullscreen;

    // Ssr records its sky table's upload, so a failure must abandon the batch
    // as well as release; the batch is dropped unsubmitted either way, which
    // releases its staging buffers before the count is read.
    let dfg_image = device
        .create_image(&ImageDesc {
            label: Some("dfg"),
            image_type: ImageType::D2,
            format: Format::Rgba8Unorm,
            extent: crcbl_hal::Extent3d::d2(1, 1),
            mip_levels: 1,
            samples: 1,
            usage: ImageUsage::SAMPLED,
        })
        .expect("an image");
    let dfg = device
        .create_image_view(&ImageViewDesc {
            label: Some("dfg"),
            image: dfg_image,
            view_type: ImageViewType::D2,
            format: Format::Rgba8Unorm,
            range: ImageSubresourceRange::all(Format::Rgba8Unorm),
        })
        .expect("a view");
    // The pipeline sets first, alone: each is built apart from its pass so a
    // renderer's build can compile it in a batch — see `ViewPipelines`.
    let ssr_pipelines = fails_cleanly_at_every_creation(
        "SsrPipelines::new",
        &recorder,
        device,
        |device| SsrPipelines::new(device, fullscreen),
        SsrPipelines::destroy,
    );
    // Then each pass over a set it takes and must release with its own
    // objects: every failure past the set's own creations is one where the
    // pass has to let go of the set it was handed.
    let ssr = fails_cleanly_at_every_creation(
        "Ssr::new",
        &recorder,
        device,
        |device| {
            let mut uploads = UploadBatch::new(device, queue, "test");
            let pipelines = SsrPipelines::new(device, fullscreen)?;
            let built = Ssr::new(device, &mut uploads, pipelines, dfg, FRAMES);
            // A failed build must abandon the batch, or a later submit would
            // copy into the objects it just released.
            if built.is_err() {
                assert!(
                    uploads.submit().is_err(),
                    "Ssr::new did not abandon its upload batch"
                );
            }
            built
        },
        Ssr::destroy,
    );
    assert!(
        ssr > ssr_pipelines,
        "`Ssr::new` created nothing past its pipelines, so it never had to release them"
    );
    fails_cleanly_at_every_creation(
        "Exposure::new",
        &recorder,
        device,
        |device| {
            let mut uploads = UploadBatch::new(device, queue, "test");
            let built = Exposure::new(device, &mut uploads, FRAMES);
            // A failed build must abandon the batch, or a later submit would
            // copy into the objects it just released.
            if built.is_err() {
                assert!(
                    uploads.submit().is_err(),
                    "Exposure::new did not abandon its upload batch"
                );
            }
            built
        },
        Exposure::destroy,
    );
    fails_cleanly_at_every_creation(
        "OcclusionPyramid::new",
        &recorder,
        device,
        |device| OcclusionPyramid::new(device, FRAMES, ForwardRenderer::build_depth_fullscreen),
        OcclusionPyramid::destroy,
    );
    fails_cleanly_at_every_creation(
        "SkyPass::new",
        &recorder,
        device,
        |device| {
            SkyPass::new(
                device,
                FRAMES,
                Format::Rgba16Float,
                Format::D32Float,
                ForwardRenderer::build_tested_fullscreen,
            )
        },
        SkyPass::destroy,
    );

    // The froxel volume names a light grid's ring and the shadow atlas in its
    // groups, so it needs both alive; neither is what is being counted.
    let stats: Vec<BufferHandle> = (0..FRAMES)
        .map(|_| {
            device
                .create_buffer(&BufferDesc {
                    label: Some("cull stats"),
                    size: u64::from(crcbl_shaders::cull::STATS_WORDS) * 4,
                    usage: BufferUsage::STORAGE | BufferUsage::TRANSFER_SRC,
                    memory: MemoryLocation::DeviceLocal,
                })
                .expect("a buffer")
        })
        .collect();
    let lights = LightGrid::new(
        device,
        &LightGridDesc {
            label: Some("lights"),
            frames: FRAMES,
            lights: 4,
            froxels: FROXEL_CAPACITY,
            stats: &stats,
        },
    )
    .expect("a light grid");
    let shadow_sampler = sampler(device);
    let volumetric_pipelines = fails_cleanly_at_every_creation(
        "VolumetricPipelines::new",
        &recorder,
        device,
        |device| VolumetricPipelines::new(device, fullscreen),
        VolumetricPipelines::destroy,
    );
    let volumetric = fails_cleanly_at_every_creation(
        "Volumetric::new",
        &recorder,
        device,
        |device| {
            let pipelines = VolumetricPipelines::new(device, fullscreen)?;
            Volumetric::new(
                device,
                pipelines,
                FRAMES,
                FROXEL_CAPACITY,
                dfg,
                shadow_sampler,
                &lights,
            )
        },
        Volumetric::destroy,
    );
    assert!(
        volumetric > volumetric_pipelines,
        "`Volumetric::new` created nothing past its pipelines, so it never had to release them"
    );

    device.destroy_sampler(shadow_sampler);
    lights.destroy(device);
    for buffer in stats {
        device.destroy_buffer(buffer);
    }
    device.destroy_image_view(dfg);
    device.destroy_image(dfg_image);
    assert_eq!(recorder.total_live_objects(), 0);
    recorder.assert_valid();
}

/// Every creation `recorder` has counted since it was last cleared, in order,
/// by kind and label — what [`Recorder::fail_create_after`] counts: a command
/// buffer is created by `finish`, which cannot fail, so it is recorded and not
/// counted.
fn creations(recorder: &Recorder) -> Vec<(ObjectKind, Option<String>)> {
    recorder
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::Created { kind, label } if *kind != ObjectKind::CommandBuffer => {
                Some((*kind, label.clone()))
            }
            _ => None,
        })
        .collect()
}

/// What a renderer's build goes on without when the device refuses it: the
/// culling counters' readback ring, whose absence `CullStatsRing::new`
/// answers with `None`.
const OPTIONAL: [&str; 1] = ["cull stats readback"];

/// **A renderer's build that fails anywhere from the camera's view-pass
/// layouts to its last creation leaks nothing** — which is the whole of the
/// mesh pass's [`pipeline_batch`] with the froxel volume's and the reflection
/// march's members in it, the camera's build with the two passes that are
/// handed a set, and everything the renderer builds after the camera.
///
/// Every member of a batch runs whatever the others returned, so a refusal
/// part way through leaves the later members' pipelines live: the mesh pass's
/// are the renderer's rollback's, the view passes' are held with their layouts
/// until each set is whole, and a set handed to the camera is the camera's to
/// release. The null device takes the serial path, which runs every member
/// just the same — see `crate::pipeline_batch`'s own tests for the parallel
/// one. A refused [`OPTIONAL`] object is not a failure: the build goes on
/// without it, and the renderer it hands back must still release whole.
#[test]
fn a_build_failing_in_its_pipeline_batch_leaks_nothing() {
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let before = recorder.total_live_objects();
    recorder.clear();
    let renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb)
        .expect("the null backend accepts every descriptor");
    let created = creations(&recorder);
    renderer.destroy(device);
    let index = |kind: ObjectKind, label: &str| {
        created
            .iter()
            .position(|(created, named)| *created == kind && named.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("the build created no {kind:?} labelled `{label}`"))
    };
    let first = index(ObjectKind::BindGroupLayout, "volumetric");
    let last = created.len() - 1;
    for (kind, label) in [
        (ObjectKind::ComputePipeline, "volumetric scatter"),
        (ObjectKind::GraphicsPipeline, "ssr"),
        (ObjectKind::GraphicsPipeline, "forward mesh"),
        (ObjectKind::GraphicsPipeline, "shadow tile clear"),
        (ObjectKind::ComputePipeline, "volumetric integrate"),
        (ObjectKind::GraphicsPipeline, "volumetric composite"),
        (ObjectKind::GraphicsPipeline, "ssr blur"),
        (ObjectKind::Image, "sky prefilter table"),
        (ObjectKind::Buffer, "volumetric params 0"),
    ] {
        assert!(
            (first..=last).contains(&index(kind, label)),
            "`{label}` is not between the view passes' layouts and the build's last \
             creation, so this window does not cover it"
        );
    }

    let mut tolerated = Vec::new();
    for (creates, failed) in created.iter().enumerate().take(last + 1).skip(first) {
        recorder.clear();
        recorder.fail_create_after(u32::try_from(creates).expect("fits"));
        let error = match ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb) {
            // An optional object the build goes on without — see
            // `OPTIONAL` — which must still release whole.
            Ok(renderer) => {
                assert!(
                    OPTIONAL.contains(&failed.1.as_deref().unwrap_or_default()),
                    "the build succeeded although creation {creates}, {failed:?}, failed"
                );
                renderer.destroy(device);
                assert_eq!(
                    recorder.total_live_objects(),
                    before,
                    "the build without creation {creates}, {failed:?}, leaked"
                );
                tolerated.push(creates);
                continue;
            }
            Err(error) => error,
        };
        assert!(
            !recorder.create_failure_pending(),
            "the build failed before reaching creation {creates}: {error}"
        );
        assert!(
            matches!(error, HalError::OutOfDeviceMemory),
            "the build did not propagate the failure at creation {creates}: {error}"
        );
        assert_eq!(
            recorder.total_live_objects(),
            before,
            "the build leaked when creation {creates}, {failed:?}, failed"
        );
    }
    eprintln!("creations {first}..={last}; built without: {tolerated:?}");
    recorder.assert_valid();
}

/// **A view made after the renderer builds both its pipeline sets or neither**:
/// `ViewPipelines::new` failed at each creation leaves nothing alive — and in
/// particular where the march's set fails after the volume's was made, which
/// it then has to release itself.
#[test]
fn a_views_pipeline_sets_release_on_every_failure() {
    let (recorder, device, _) = open();
    let device = device.as_ref();
    let fullscreen = ForwardRenderer::build_fullscreen;
    let volumetric = fails_cleanly_at_every_creation(
        "VolumetricPipelines::new",
        &recorder,
        device,
        |device| VolumetricPipelines::new(device, fullscreen),
        VolumetricPipelines::destroy,
    );
    let both = fails_cleanly_at_every_creation(
        "ViewPipelines::new",
        &recorder,
        device,
        ViewPipelines::new,
        |pipelines, device| {
            pipelines.ssr.destroy(device);
            pipelines.volumetric.destroy(device);
        },
    );
    assert!(
        both > volumetric,
        "`ViewPipelines::new` created nothing past the volume's set, so no failure \
         had a finished set to release"
    );
    assert_eq!(recorder.total_live_objects(), 0);
}
