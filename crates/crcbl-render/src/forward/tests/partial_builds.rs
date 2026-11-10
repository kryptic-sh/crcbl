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
/// build is destroyed and checked for leaks too, so a `destroy` that releases
/// something the build did not create — the caller's sampler, say — shows up.
///
/// **What this cannot see is a handle released twice.** The null backend's
/// `destroy_*` on a dead handle does nothing and records nothing, so a double
/// destroy, on a failure path or the success path, leaves the count where a
/// single one would — see `docs/backlog.md`.
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

/// Releases whatever a [`view::ContentPasses`] holds.
fn destroy_content(passes: view::ContentPasses, device: &dyn Device) {
    if let Some(pass) = passes.grass {
        pass.destroy(device);
    }
    if let Some(pass) = passes.water {
        pass.destroy(device);
    }
}

/// **A first-use content build releases everything on every failure** —
/// the water passes alone, the grass passes alone, and both out of one
/// batch, where a refusal in either set leaves the other's pipelines live
/// and a failure past the batch leaves the other pass's whole set to release.
#[test]
fn the_content_passes_release_on_every_failure() {
    let (recorder, device, _) = open();
    let device = device.as_ref();
    let shadow_sampler = sampler(device);
    let build = |water, grass| {
        move |device: &dyn Device| {
            view::ContentPasses::new(device, FRAMES, shadow_sampler, water, grass, &mut || {})
        }
    };
    let water = fails_cleanly_at_every_creation(
        "ContentPasses::new (water)",
        &recorder,
        device,
        build(true, false),
        destroy_content,
    );
    // The draw batch's members are created side by side, so this also covers a
    // refusal that leaves batch siblings live.
    let grass = fails_cleanly_at_every_creation(
        "ContentPasses::new (grass)",
        &recorder,
        device,
        build(false, true),
        destroy_content,
    );
    let both = fails_cleanly_at_every_creation(
        "ContentPasses::new (water and grass)",
        &recorder,
        device,
        build(true, true),
        destroy_content,
    );
    assert_eq!(
        both,
        water + grass,
        "the build of both made other than each build's creations together, so some \
         failure of one set beside the other was not checked"
    );
    // Neither asked for builds nothing, and fails nowhere.
    let before = recorder.total_live_objects();
    let nothing = build(false, false)(device).expect("nothing to create");
    assert!(nothing.water.is_none() && nothing.grass.is_none());
    assert_eq!(recorder.total_live_objects(), before);
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
            let pipelines = SsrPipelines::new(device, fullscreen)?;
            // The DFG view stands in for the renderer's sky table too: `Ssr`
            // borrows both and releases neither.
            Ssr::new(device, pipelines, dfg, dfg, FRAMES)
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
/// handed a set, and everything the renderer builds after the camera — and at
/// the renderer's sky prefilter table, which the camera's march borrows.
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
        (ObjectKind::Buffer, "ssr params"),
        (ObjectKind::Buffer, "volumetric params 0"),
    ] {
        assert!(
            (first..=last).contains(&index(kind, label)),
            "`{label}` is not between the view passes' layouts and the build's last \
             creation, so this window does not cover it"
        );
    }
    // The renderer's sky prefilter table, which every view's march borrows, is
    // uploaded before that window: its staging buffer, image and view are
    // failed too, since a view's build no longer creates them.
    let table = index(ObjectKind::Image, "sky prefilter table");
    let sky_table = table - 1..=table + 1;
    for creates in sky_table.clone() {
        assert!(
            created[creates]
                .1
                .as_deref()
                .is_some_and(|label| label.starts_with("sky prefilter table")),
            "creation {creates}, {:?}, is not the sky prefilter table's",
            created[creates]
        );
    }

    let mut tolerated = Vec::new();
    for creates in sky_table.chain(first..=last) {
        let failed = &created[creates];
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
/// particular where a member fails after its batch has made others, which it
/// then has to release itself.
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
        |device| ViewPipelines::new(device, &mut || {}),
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

/// The four settings-driven passes the default build leaves to a frame, asked
/// for at once — see `lazy_passes.rs`: the contact march and the bloom chain by
/// their effect bits, the cheap antialiasing tier with CMAA2 forced off, and
/// the upscale by a render scale below one.
fn ask_for_every_lazy_pass(renderer: &mut ForwardRenderer) {
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(
                RenderEffects::BLOOM
                    .union(RenderEffects::CONTACT_SHADOWS)
                    .union(RenderEffects::ANTIALIASING),
                Some(true),
            )
            .force(RenderEffects::CMAA2, Some(false)),
        ..EffectRequest::default()
    });
    renderer.set_render_scale(0.5);
}

/// Every object `recorder` holds alive but the readbacks in flight: a frame
/// requests the culling counters' readback before any view begins, so a frame
/// that fails in a view's build still has one out, which the ring resolves
/// when its slot comes round.
fn besides_readbacks(recorder: &Recorder) -> usize {
    recorder.total_live_objects() - recorder.live_objects(ObjectKind::Readback)
}

/// A frame of [`TEST_EXTENT`] under the default camera and sun, begun and
/// nothing more: what a game loop's `begin_frame` does before it records.
fn begin(device: &dyn Device, renderer: &mut ForwardRenderer) -> Result<(), HalError> {
    renderer.begin_frame(
        device,
        &Camera::default(),
        &DirectionalLight::default(),
        TEST_EXTENT,
    )
}

/// **A lazy build that fails part-way through a frame leaks nothing, and the
/// next frame builds the rest** — `View::build_passes` failed inside
/// `begin_frame`, which is where a game meets it.
///
/// The failure lands on the bloom chain's composite pipeline: after the
/// contact march was built whole, which the view keeps, and after the chain's
/// own layouts and first pipelines, which `Bloom::new` must release. The frame
/// reports the injected error, the view holds the march and no chain, and the
/// device holds exactly the march's objects more than before, readbacks
/// aside — see [`besides_readbacks`]. The next frame, with creation succeeding
/// again, builds what is missing; a lap of the ring later the device holds
/// what the same lap of clean frames leaves — so nothing failed was built twice
/// or left half-built — and the renderer still releases whole.
///
/// A lap and not one frame, because the failed frame took a slot of the ring:
/// the retry draws in a different slot from the clean run's first lazy frame,
/// and each slot caches its own bind groups the first time a frame draws there
/// under these passes.
#[test]
fn a_lazy_build_failing_inside_a_frame_is_finished_by_the_next() {
    // The clean run: what the first frame asking for every lazy pass creates,
    // and what the device holds a lap of the ring later. Both runs draw a
    // default frame first, which retires the build's uploads, so the counts
    // below move only with the lazy build.
    let (clean, clean_device, clean_queue) = open();
    let clean_device = clean_device.as_ref();
    let mut renderer =
        ForwardRenderer::new(clean_device, clean_queue, Format::Rgba8UnormSrgb).expect("built");
    frame(clean_device, &mut renderer, clean_queue).release(clean_device);
    ask_for_every_lazy_pass(&mut renderer);
    clean.clear();
    for _ in 0..FRAMES {
        frame(clean_device, &mut renderer, clean_queue).release(clean_device);
    }
    let built = clean.total_live_objects();
    let created = creations(&clean);
    renderer.destroy(clean_device);
    let position = |label: &str| {
        created
            .iter()
            .position(|(_, named)| named.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("the lazy frame created nothing labelled `{label}`"))
    };
    let failing = created
        .iter()
        .position(|created| {
            *created == (ObjectKind::GraphicsPipeline, Some("bloom composite".into()))
        })
        .expect("the lazy frame built the bloom chain's composite");
    assert!(
        position("contact shadow params") < position("bloom chain")
            && position("bloom chain") < failing,
        "the failure does not land in the bloom chain after the contact march was \
         built, so it is not a partial build: {created:?}"
    );

    // What the contact march alone keeps alive on this device.
    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let empty = recorder.total_live_objects();
    let march = ContactShadows::new(device, FRAMES, ForwardRenderer::build_fullscreen)
        .expect("the null backend accepts every descriptor");
    let march_objects = recorder.total_live_objects() - empty;
    march.destroy(device);
    assert_eq!(recorder.total_live_objects(), empty);

    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    frame(device, &mut renderer, queue).release(device);
    ask_for_every_lazy_pass(&mut renderer);
    let before = besides_readbacks(&recorder);
    recorder.fail_create_after(u32::try_from(failing).expect("fits"));
    let error =
        begin(device, &mut renderer).expect_err("the frame went on although its lazy build failed");
    assert!(
        !recorder.create_failure_pending(),
        "the frame failed before reaching creation {failing}: {error}"
    );
    assert!(
        matches!(error, HalError::OutOfDeviceMemory),
        "the frame did not report the injected failure: {error}"
    );
    assert!(
        renderer.primary.contact_shadows.is_some() && renderer.primary.bloom.is_none(),
        "the view does not hold the march built before the failure and no chain"
    );
    assert_eq!(
        besides_readbacks(&recorder),
        before + march_objects,
        "the failed frame left something besides the contact march alive"
    );

    for _ in 0..FRAMES {
        frame(device, &mut renderer, queue).release(device);
    }
    assert!(
        renderer.primary.bloom.is_some()
            && renderer.primary.fxaa.is_some()
            && renderer.primary.upscale.is_some(),
        "the next frame did not build what the failed one left"
    );
    assert_eq!(
        recorder.total_live_objects(),
        built,
        "a lap from the retried frame left a different device from a lap of clean frames"
    );

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}

/// The buffers one ring slot holds the water bodies' mesh in — see
/// `WaterBodies::begin_frame`.
const WATER_MESH: [&str; 3] = ["water vertices", "water indices", "water media"];

/// Gives `renderer` a body of water and a field of grass, whose passes its
/// next frame builds out of one `View::build_content` call — see
/// `lazy_content.rs`.
fn set_water_and_grass(device: &dyn Device, queue: QueueHandle, renderer: &mut ForwardRenderer) {
    renderer
        .set_water(&[super::lazy_content::pond()])
        .expect("a square meshes");
    renderer
        .set_grass(device, queue, Some(&super::lazy_content::lawn()))
        .expect("the null backend uploads every map");
}

/// **A content build that fails part-way through a frame leaks nothing, and
/// the next frame builds it whole** — `View::build_content` failed inside
/// `begin_frame`, as [`a_lazy_build_failing_inside_a_frame_is_finished_by_the_next`]
/// fails `View::build_passes`.
///
/// The failure lands on the grass passes' first uniform buffer: after both
/// pipeline sets and the whole water pass were built, and after the grass
/// pass's samplers, which `Grass::new` must release with its set. Unlike the
/// contact march there, the water pass built before it is **not** kept: one
/// call builds both, all or nothing — see `ContentPasses::new` — so the frame
/// reports the injected error and the view holds neither pass. What the frame
/// does keep is its slot's copy of the bodies' mesh, [`WATER_MESH`], which
/// `WaterBodies::begin_frame` uploaded before any view began and which that
/// slot draws from when it comes round again; the device holds exactly that
/// more than before, readbacks aside — see [`besides_readbacks`]. The next
/// frame builds both passes; a lap of the ring later the device holds what the
/// same lap of clean frames leaves, and the renderer still releases whole.
#[test]
fn a_content_build_failing_inside_a_frame_is_finished_by_the_next() {
    // The clean run, on the sibling test's terms: a default frame first to
    // retire the build's uploads, then the content, then a lap of frames.
    let (clean, clean_device, clean_queue) = open();
    let clean_device = clean_device.as_ref();
    let mut renderer =
        ForwardRenderer::new(clean_device, clean_queue, Format::Rgba8UnormSrgb).expect("built");
    frame(clean_device, &mut renderer, clean_queue).release(clean_device);
    set_water_and_grass(clean_device, clean_queue, &mut renderer);
    clean.clear();
    for _ in 0..FRAMES {
        frame(clean_device, &mut renderer, clean_queue).release(clean_device);
    }
    let built = clean.total_live_objects();
    let created = creations(&clean);
    renderer.destroy(clean_device);
    let position = |kind: ObjectKind, label: &str| {
        created
            .iter()
            .position(|(created, named)| *created == kind && named.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("the content frame created no {kind:?} labelled `{label}`"))
    };
    let failing = position(ObjectKind::Buffer, "grass params");
    // The batch's last member, then the water pass, then the grass pass's
    // samplers: every pipeline and the whole water pass precede the failure.
    assert!(
        position(ObjectKind::ComputePipeline, "grass generate")
            < position(ObjectKind::Buffer, "water params")
            && position(ObjectKind::Buffer, "water params")
                < position(ObjectKind::Sampler, "grass card")
            && position(ObjectKind::Sampler, "grass card") < failing,
        "the failure does not land in the grass pass after the water pass was built, so \
         it is not a partial build: {created:?}"
    );

    let (recorder, device, queue) = open();
    let device = device.as_ref();
    let mut renderer = ForwardRenderer::new(device, queue, Format::Rgba8UnormSrgb).expect("built");
    frame(device, &mut renderer, queue).release(device);
    set_water_and_grass(device, queue, &mut renderer);
    let before = besides_readbacks(&recorder);
    recorder.clear();
    recorder.fail_create_after(u32::try_from(failing).expect("fits"));
    let error = begin(device, &mut renderer)
        .expect_err("the frame went on although its content build failed");
    let tried = creations(&recorder);
    for label in WATER_MESH {
        assert_eq!(
            tried
                .iter()
                .filter(|made| **made == (ObjectKind::Buffer, Some(label.into())))
                .count(),
            1,
            "the failed frame did not upload the bodies' mesh into its slot once: {tried:?}"
        );
    }
    assert!(
        !recorder.create_failure_pending(),
        "the frame failed before reaching creation {failing}: {error}"
    );
    assert!(
        matches!(error, HalError::OutOfDeviceMemory),
        "the frame did not report the injected failure: {error}"
    );
    assert!(
        renderer.primary.water.is_none() && renderer.primary.grass.is_none(),
        "the view holds a content pass although the build of both failed"
    );
    assert_eq!(
        besides_readbacks(&recorder),
        before + WATER_MESH.len(),
        "the failed frame left something besides its slot's mesh alive"
    );

    for _ in 0..FRAMES {
        frame(device, &mut renderer, queue).release(device);
    }
    assert!(
        renderer.primary.water.is_some() && renderer.primary.grass.is_some(),
        "the next frame did not build what the failed one left"
    );
    assert_eq!(
        recorder.total_live_objects(),
        built,
        "a lap from the retried frame left a different device from a lap of clean frames"
    );

    renderer.destroy(device);
    assert_eq!(recorder.total_live_objects(), 0, "the renderer leaked");
    recorder.assert_valid();
}
