use super::*;
use crcbl_hal::null::NullInstance;
use crcbl_hal::{DeviceDesc, Features, Instance, QueueKind};

fn open() -> (Box<dyn Device>, QueueHandle) {
    let instance = NullInstance::gpu_driven();
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::GPU_DRIVEN,
            optional_features: Features::PUSH_CONSTANTS,
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    (device, queue)
}

/// [`open`] with a recorder attached, for the tests whose claim is about
/// what the renderer did rather than what it returned.
fn open_recorded() -> (crcbl_hal::null::Recorder, Box<dyn Device>, QueueHandle) {
    let recorder = crcbl_hal::null::Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::GPU_DRIVEN,
            optional_features: Features::PUSH_CONSTANTS,
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    (recorder, device, queue)
}

/// Bytes written into the current frame's vertex and index rings, read off
/// the recorded stream.
///
/// Zero for a ring nothing was written to, which is the case an `Ok` from
/// [`UiRenderer::begin_frame`] cannot tell apart from a full upload.
fn uploaded(recorder: &crcbl_hal::null::Recorder, renderer: &UiRenderer) -> (usize, usize) {
    use crcbl_hal::null::Event;

    let vertices = renderer.vertex_buffers[renderer.frame];
    let indices = renderer.index_buffers[renderer.frame];
    let mut written = (0, 0);
    for event in recorder.events() {
        if let Event::BufferWritten {
            buffer,
            offset,
            len,
        } = event
        {
            assert_eq!(offset, 0, "a ring is written from its start");
            if buffer == vertices {
                written.0 += len;
            } else if buffer == indices {
                written.1 += len;
            }
        }
    }
    written
}

/// Everything [`UiRenderer::new`] created, [`UiRenderer::destroy`] hands
/// back.
///
/// The recorder is what makes the second half of that a claim: without it
/// a leaked sampler, pipeline layout or bind-group layout is invisible with
/// no GPU, and this test asserted nothing at all.
#[test]
fn ui_renderer_builds_and_leaks_nothing() {
    let (recorder, device, queue) = open_recorded();
    let before = recorder.total_live_objects();
    let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
        .expect("the null backend accepts everything");
    assert!(
        recorder.total_live_objects() > before,
        "a renderer that created nothing would also leak nothing"
    );
    renderer.destroy(device.as_ref());
    assert_eq!(
        recorder.total_live_objects(),
        before,
        "destroy must give back every object new took"
    );
    recorder.assert_valid();
}

/// The glyph atlas upload moved to [`crate::texture`] and must not have
/// changed on the way: one byte per texel, the atlas's own extent, and
/// `Undefined → TransferDst → ShaderRead` around the copy.
///
/// The image's *format* is not observable through the recorder — it logs a
/// kind and a label, not the descriptor — so the staging write's length
/// stands in for it: `R8Unorm` writes `width * height`, and the same call
/// with `Rgba8Unorm` would write four times that.
///
/// The atlas is 768 texels wide, which is already a multiple of Tier A's
/// 4-byte copy alignment, so *this* upload pads nothing and the numbers are
/// spelled out rather than recomputed. The padding itself is exercised in
/// [`crate::texture`]'s own tests, against Tier B's 256-byte alignment.
#[test]
fn the_glyph_atlas_is_still_an_r8_upload_at_the_same_pitch() {
    use crcbl_hal::null::{Command, Event};
    use crcbl_hal::{Extent3d, Offset3d, ResourceState};

    let recorder = crcbl_hal::null::Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::GPU_DRIVEN,
            optional_features: Features::PUSH_CONSTANTS,
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");

    let (atlas_w, atlas_h, atlas_pixels) = FontAtlas::built_in().glyph_bitmap();
    assert_eq!((atlas_w, atlas_h), (768, 13));
    assert_eq!(atlas_pixels.len(), 768 * 13);
    assert_eq!(
        768 % device.caps().limits.optimal_buffer_copy_offset_alignment,
        0,
        "the pitch below is the unpadded width only because the row is already aligned"
    );

    let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
        .expect("the null backend accepts everything");

    let written = recorder
        .events()
        .into_iter()
        .find_map(|event| match event {
            Event::BufferWritten { len, .. } => Some(len),
            _ => None,
        })
        .expect("the atlas staging buffer is written before any frame buffer");
    assert_eq!(
        written,
        768 * 13,
        "one byte per texel: the same call with an Rgba8Unorm atlas would write four times this"
    );

    let commands = recorder.commands();
    let copy = commands
        .iter()
        .find_map(|command| match command {
            Command::CopyBufferToImage(copy) => Some(*copy),
            _ => None,
        })
        .expect("the atlas is uploaded with one buffer-to-image copy");
    assert_eq!(
        copy.buffer_row_length, 768,
        "R8 is one byte per texel, so the texel pitch equals the byte pitch"
    );
    assert_eq!(copy.buffer_image_height, atlas_h);
    assert_eq!(copy.image_extent, Extent3d::d2(atlas_w, atlas_h));
    assert_eq!(copy.image_offset, Offset3d { x: 0, y: 0, z: 0 });

    let transitions: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Barrier { images, .. } => Some(images.clone()),
            _ => None,
        })
        .flatten()
        .map(|barrier| (barrier.from, barrier.to))
        .collect();
    assert_eq!(
        transitions,
        [
            (ResourceState::Undefined, ResourceState::TransferDst),
            (ResourceState::TransferDst, ResourceState::ShaderRead),
            (ResourceState::Undefined, ResourceState::TransferDst),
            (ResourceState::TransferDst, ResourceState::ShaderRead),
            (ResourceState::Undefined, ResourceState::TransferDst),
            (ResourceState::TransferDst, ResourceState::ShaderRead),
        ],
        "the two atlases and the glyph pages are the only barriers the UI renderer's \
         construction records"
    );

    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// An empty draw list writes no bytes and leaves the frame with nothing to
/// draw.
///
/// The `Ok` this used to assert comes back from an upload of any size,
/// including one that wrote the *previous* frame's geometry again and left
/// its counts in place for the draw call to read.
#[test]
fn an_empty_draw_list_uploads_no_bytes_and_leaves_the_counts_at_zero() {
    let (recorder, device, queue) = open_recorded();
    let mut renderer =
        UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();
    // The atlas upload is `new`'s, not this frame's.
    recorder.clear();

    let dl = DrawList::new();
    renderer
        .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
        .expect("empty draw list upload should succeed");

    assert_eq!(uploaded(&recorder, &renderer), (0, 0));
    assert_eq!(renderer.last_vertex_count[renderer.frame], 0);
    assert_eq!(renderer.last_index_count[renderer.frame], 0);
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// Both primitives really reach the rings, and they are not the same
/// geometry.
///
/// Two tests here once asserted that `begin_frame` returned `Ok`, which a
/// `begin_frame` that returned before touching a buffer does too. The
/// observable is the byte count the recorder saw, derived from the
/// tessellation rather than spelled out — a literal would rot the first
/// time [`Vertex2d`] grew a field. And the two cases must disagree: two
/// primitives asserting one number is one case written twice.
#[test]
fn a_rect_and_a_line_of_text_each_upload_exactly_the_geometry_they_tessellate_to() {
    let atlas = FontAtlas::built_in();

    let mut rect = DrawList::new();
    rect.rect(
        glam::Vec2::new(10.0, 20.0),
        glam::Vec2::new(110.0, 120.0),
        [1.0, 0.0, 0.0, 1.0],
    );
    let mut text = DrawList::new();
    text.text(
        glam::Vec2::new(10.0, 10.0),
        "Hello",
        [1.0, 1.0, 1.0, 1.0],
        14.0,
    );

    let mut sizes = Vec::new();
    for (what, dl) in [("a rect", &rect), ("a line of text", &text)] {
        let (recorder, device, queue) = open_recorded();
        let mut renderer =
            UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
        recorder.clear();

        renderer
            .begin_frame(device.as_ref(), dl, &atlas, 1.0)
            .expect("upload should succeed");

        let (vertices, indices) = dl.to_triangles(Some(&atlas), None, 1.0);
        let expected = (
            vertices.len() * std::mem::size_of::<Vertex2d>(),
            indices.len() * std::mem::size_of::<u32>(),
        );
        assert!(
            expected.0 > 0 && expected.1 > 0,
            "{what} tessellates to nothing, so this case asserts nothing"
        );
        assert_eq!(uploaded(&recorder, &renderer), expected, "{what}");
        assert_eq!(renderer.last_vertex_count[renderer.frame], vertices.len());
        assert_eq!(renderer.last_index_count[renderer.frame], indices.len());

        sizes.push(expected);
        renderer.destroy(device.as_ref());
        recorder.assert_valid();
    }

    assert_eq!(sizes.len(), 2, "both primitives were measured");
    assert_ne!(
        sizes[0], sizes[1],
        "one quad and five glyphs are not the same geometry"
    );
}

/// A UI that has not changed must not churn the GPU: the byte counts and
/// the element counts used to be compared against each other, so both ring
/// buffers *and* the frame bind group were destroyed and recreated every
/// single frame in steady state.
#[test]
fn a_steady_state_frame_recreates_nothing() {
    let recorder = crcbl_hal::null::Recorder::new();
    let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::GPU_DRIVEN,
            optional_features: Features::PUSH_CONSTANTS,
            compatible_surface: None,
        })
        .expect("the null backend always opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    let mut renderer =
        UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let atlas = FontAtlas::built_in();
    let mut dl = DrawList::new();
    dl.text(
        glam::Vec2::new(10.0, 10.0),
        "steady",
        [1.0, 1.0, 1.0, 1.0],
        14.0,
    );

    // Two frames to fill both slots of the ring, then measure.
    for _ in 0..FRAMES_IN_FLIGHT {
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("upload");
    }
    let buffers = renderer.vertex_buffers.clone();
    let groups = renderer.frame_groups.clone();
    let settled = recorder.total_live_objects();

    for _ in 0..8 {
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("upload");
    }
    assert_eq!(
        recorder.total_live_objects(),
        settled,
        "an unchanged draw list must not allocate"
    );
    assert_eq!(
        renderer.vertex_buffers, buffers,
        "the vertex ring must be reused, not reallocated"
    );
    assert_eq!(
        renderer.frame_groups, groups,
        "the frame bind group only changes when its vertex buffer does"
    );

    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **The counters are the one draw this pass records and the triangles it
/// wrote the indices for**, against two lists whose triangle counts differ.
///
/// One rectangle is two triangles; a rectangle and a string are more, and
/// the number is the index list's own length rather than a per-glyph
/// estimate — so a counter that guessed, or that reported the vertex count,
/// fails on both. An empty list records no pass at all and the counters say
/// zero *and know it*, which is the value `indirect` is not.
#[test]
fn the_counters_are_the_one_draw_and_the_indices_it_covers() {
    let (device, queue) = open();
    let mut renderer =
        UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();

    let mut one_rect = DrawList::new();
    one_rect.rect(
        glam::Vec2::ZERO,
        glam::Vec2::new(10.0, 10.0),
        [1.0, 1.0, 1.0, 1.0],
    );
    renderer
        .begin_frame(device.as_ref(), &one_rect, &atlas, 1.0)
        .expect("upload");
    let counters = renderer.counters();
    assert_eq!(counters.draws, 1, "one `draw_indexed` for the whole list");
    assert_eq!(counters.instances, 1);
    assert_eq!(counters.drawn, Some(1));
    assert_eq!(counters.triangles, Some(2), "a quad is two triangles");

    let mut with_text = one_rect.clone();
    with_text.text(
        glam::Vec2::new(4.0, 4.0),
        "counters",
        [1.0, 1.0, 1.0, 1.0],
        14.0,
    );
    renderer
        .begin_frame(device.as_ref(), &with_text, &atlas, 1.0)
        .expect("upload");
    let richer = renderer.counters();
    assert_eq!(
        richer.draws, 1,
        "still one draw, however much is in the list"
    );
    // The index list the pass actually built, so this is the pass's own
    // arithmetic and not a second count of the glyphs.
    let (_, indices) = with_text.to_triangles(Some(&atlas), None, 1.0);
    assert_eq!(richer.triangles, Some(indices.len() as u64 / 3));
    assert!(
        richer.triangles > counters.triangles,
        "a longer list must move the counter: {:?} against {:?}",
        richer.triangles,
        counters.triangles,
    );

    renderer
        .begin_frame(device.as_ref(), &DrawList::new(), &atlas, 1.0)
        .expect("upload");
    assert_eq!(
        renderer.counters(),
        crate::counters::FrameCounters::default()
    );

    renderer.destroy(device.as_ref());
}

/// The ring still grows when a frame genuinely needs more room, and the old
/// buffer is released rather than leaked.
#[test]
fn a_bigger_draw_list_grows_the_ring_once() {
    let (device, queue) = open();
    let mut renderer =
        UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();

    let mut small = DrawList::new();
    small.rect(
        glam::Vec2::ZERO,
        glam::Vec2::new(10.0, 10.0),
        [1.0, 1.0, 1.0, 1.0],
    );
    renderer
        .begin_frame(device.as_ref(), &small, &atlas, 1.0)
        .expect("upload");
    let before = renderer.vertex_capacity[renderer.frame];

    let mut big = DrawList::new();
    for index in 0..512 {
        let x = index as f32;
        big.rect(
            glam::Vec2::new(x, 0.0),
            glam::Vec2::new(x + 1.0, 1.0),
            [1.0, 1.0, 1.0, 1.0],
        );
    }
    // Two frames so the same ring slot comes round again.
    for _ in 0..FRAMES_IN_FLIGHT {
        renderer
            .begin_frame(device.as_ref(), &big, &atlas, 1.0)
            .expect("upload");
    }
    assert!(
        renderer.vertex_capacity[renderer.frame] > before,
        "the ring must grow when the frame no longer fits"
    );
    renderer.destroy(device.as_ref());
}

/// A device that reports no [`Features::PUSH_CONSTANTS`] — what a browser
/// is, and what this pass used to need a second shader artifact for.
fn open_portable() -> (Box<dyn Device>, QueueHandle) {
    let instance = NullInstance::portable();
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::COMPUTE,
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("the tier B null adapter opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    (device, queue)
}

/// **Both devices build the same renderer**, each with one constants buffer
/// per frame in flight. A device that reports push constants gets no
/// different treatment from one that does not, which is the whole of what
/// deleting `ConstantDelivery` was for: the two used to differ in the
/// pipeline layout, the bind-group layout, the buffers allocated, the
/// commands recorded *and* the shader artifact resolved.
#[test]
fn push_constants_or_not_the_renderer_is_the_same() {
    for (device, queue) in [open(), open_portable()] {
        let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb)
            .expect("neither device is refused");
        assert_eq!(
            renderer.constant_buffers.len(),
            FRAMES_IN_FLIGHT,
            "one constants buffer per frame in flight, or two frames share one"
        );
        renderer.destroy(device.as_ref());
    }
}

/// The renderer builds, uploads and tears down with no GPU and no leak on a
/// device with no push constants — the path that was once an early
/// `return Err`, and then a second shader artifact.
#[test]
fn the_portable_renderer_leaks_nothing() {
    let recorder = crcbl_hal::null::Recorder::new();
    let instance = NullInstance::portable().with_recorder(recorder.clone());
    let adapter = instance.adapters().remove(0);
    let device = instance
        .create_device(&DeviceDesc {
            label: None,
            adapter: adapter.id,
            required_features: Features::COMPUTE,
            optional_features: Features::empty(),
            compatible_surface: None,
        })
        .expect("the portable null adapter opens");
    let queue = device.queue(QueueKind::Graphics).expect("always present");
    let before = recorder.total_live_objects();

    let mut renderer =
        UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();
    let mut dl = DrawList::new();
    dl.text(glam::Vec2::new(4.0, 4.0), "score", [1.0; 4], 14.0);
    for _ in 0..FRAMES_IN_FLIGHT * 2 {
        renderer
            .begin_frame(device.as_ref(), &dl, &atlas, 1.0)
            .expect("upload");
    }
    renderer.destroy(device.as_ref());
    assert_eq!(recorder.total_live_objects(), before);
    recorder.assert_valid();
}

// -----------------------------------------------------------------------
// A paused frame
// -----------------------------------------------------------------------

/// A pause menu, laid out for `extent`, as `apps/*/src/menu.rs` builds one.
fn pause_menu() -> crcbl_ui::menu::Menu {
    use crcbl_ui::menu::{Menu, MenuItem};
    Menu::new(
        "PAUSED",
        vec![
            MenuItem::new(1, "RESUME", "ESC"),
            MenuItem::new(2, "QUIT", ""),
        ],
    )
}

/// A draw list shaped like a paused frame, as the engine's loop builds one: a
/// HUD bar, the cut, then the whole menu — its art and its text — drawn with
/// the renderer's own skin.
fn paused_list(ui: &UiRenderer, extent: (u32, u32), atlas: &FontAtlas) -> DrawList {
    let mut list = DrawList::new();
    list.rect(
        glam::Vec2::new(0.0, 0.0),
        glam::Vec2::new(64.0, 8.0),
        [1.0; 4],
    );
    list.begin_overlay();
    let panel = pause_menu();
    panel.render(&mut list, &panel.layout(extent, atlas), ui.menu_skin());
    list
}

/// **A paused frame is the two halves of one upload drawn back to back, the
/// menu's art in the second** — and the two halves partition the index
/// buffer.
///
/// The pass labels come out of the compiled graph in execution order, and
/// the index ranges come out of the recorded draw calls — so a swap of the
/// two `add_segment` calls fails the labels, and a range that overlapped or
/// left a gap fails the arithmetic. The ranges are asserted against each
/// other and against the frame's own index count rather than against
/// literals, which is what keeps the test about the partition instead of
/// about the glyph layout.
#[test]
fn a_paused_frame_draws_both_halves_of_one_upload_with_the_menu_art_above_the_cut() {
    use crate::graph::{CompiledPass, RenderGraph};
    use crate::transient::{TransientImageDesc, TransientPool};
    use crcbl_hal::null::{Command, Event};
    use crcbl_hal::{CommandEncoderDesc, Format as HalFormat, ImageUsage};

    const EXTENT: (u32, u32) = (128, 96);

    let (recorder, device, queue) = open_recorded();
    let mut pool = TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let atlas = FontAtlas::built_in();
    let list = paused_list(&ui, EXTENT, &atlas);
    ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
        .expect("upload");

    let total = ui.last_index_count[ui.frame] as u32;
    let split = ui.last_overlay_index[ui.frame] as u32;
    assert!(
        split > 0 && split < total,
        "the fixture must put geometry on both sides of the cut: {split} of {total}"
    );
    // The menu's frame is textured quads in the overlay half, off the
    // renderer's own atlas: every image vertex is above the cut.
    let triangles = list.to_triangles_split(Some(&atlas), None, 1.0);
    let image_vertices: Vec<u32> = triangles.indices[..]
        .iter()
        .copied()
        .filter(|&index| {
            triangles.vertices[index as usize].primitive()
                == Some(crcbl_ui::draw_list::Primitive::Image)
        })
        .collect();
    assert!(!image_vertices.is_empty(), "the menu drew no art");
    assert!(
        triangles.indices[..split as usize]
            .iter()
            .all(|&index| !image_vertices.contains(&index)),
        "menu art landed below the cut"
    );

    // The uploads are start-up and per-frame CPU work; what is under test is
    // the passes the frame records.
    recorder.clear();

    let mut graph = RenderGraph::new(queue);
    let target = graph.create_image(
        "target",
        TransientImageDesc::new(
            EXTENT,
            HalFormat::Bgra8UnormSrgb,
            ImageUsage::COLOR_ATTACHMENT,
        ),
    );
    ui.add_passes(&mut graph, target, EXTENT);
    let compiled = graph.compile(&pool).expect("a legal frame");

    let labels: Vec<&str> = compiled.passes().iter().map(CompiledPass::label).collect();
    assert_eq!(
        labels,
        ["ui-composite", "ui-overlay"],
        "the HUD half, then the overlay half, and nothing between them"
    );

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("paused frame"),
        queue,
    });
    compiled
        .execute(device.as_ref(), &mut pool, encoder.as_mut(), None)
        .expect("the graph executed");

    // The recorder sees a command stream only once the encoder is finished
    // — the shape `tests/ui_pass_stream.rs` reads it in.
    let commands = encoder.finish().expect("recording succeeded");

    let drawn: Vec<std::ops::Range<u32>> = recorder
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            Command::DrawIndexed { indices, .. } => Some(indices),
            _ => None,
        })
        .collect();
    assert_eq!(
        drawn,
        vec![0..split, split..total],
        "the two halves must partition the frame's one index buffer"
    );

    // Both halves are drawn out of the *same* upload — the assertion that
    // separates a split pass from a second tessellation.
    let writes = recorder
        .events()
        .into_iter()
        .filter(|event| matches!(event, Event::BufferWritten { .. }))
        .count();
    assert_eq!(
        writes, 2,
        "one viewport-constants write per pass and no second geometry \
         upload: the tessellation happened before the frame"
    );

    let counters = ui.counters();
    assert_eq!(counters.draws, 2, "one draw per half");
    assert_eq!(counters.drawn, Some(2));
    assert_eq!(
        counters.triangles,
        Some(u64::from(total) / 3),
        "and every triangle in the buffer is in one half or the other"
    );

    device.destroy_command_buffer(commands);
    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
}

/// A frame with nothing above the cut records **one** pass, named as it
/// always was — the unpaused case every sample spends its life in, and the
/// reason every existing `contains("ui-composite")` assertion still holds.
#[test]
fn an_uncut_draw_list_is_still_one_ui_composite_pass() {
    use crate::graph::{CompiledPass, RenderGraph};
    use crate::transient::{TransientImageDesc, TransientPool};
    use crcbl_hal::{Format as HalFormat, ImageUsage};

    const EXTENT: (u32, u32) = (128, 96);

    let (device, queue) = open();
    let mut pool = TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();
    let mut list = DrawList::new();
    list.text(glam::Vec2::new(4.0, 4.0), "SCORE", [1.0; 4], 14.0);
    ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
        .expect("upload");

    let mut graph = RenderGraph::new(queue);
    let target = graph.create_image(
        "target",
        TransientImageDesc::new(
            EXTENT,
            HalFormat::Bgra8UnormSrgb,
            ImageUsage::COLOR_ATTACHMENT,
        ),
    );
    ui.add_passes(&mut graph, target, EXTENT);
    let compiled = graph.compile(&pool).expect("a legal frame");

    let labels: Vec<&str> = compiled.passes().iter().map(CompiledPass::label).collect();
    assert_eq!(labels, ["ui-composite"]);
    assert_eq!(ui.counters().draws, 1, "one half, one draw");

    drop(compiled);
    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
}

/// A frame that grows its ring rebuilds a bind group that still names the
/// constants buffer — the entry `new` and `begin_frame` used to spell twice,
/// and could therefore spell differently.
#[test]
fn growing_the_ring_keeps_the_constants_bound() {
    let (device, queue) = open_portable();
    let mut renderer =
        UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();

    let mut big = DrawList::new();
    for index in 0..512 {
        let x = index as f32;
        big.rect(
            glam::Vec2::new(x, 0.0),
            glam::Vec2::new(x + 1.0, 1.0),
            [1.0; 4],
        );
    }
    for _ in 0..FRAMES_IN_FLIGHT {
        renderer
            .begin_frame(device.as_ref(), &big, &atlas, 1.0)
            .expect("upload");
    }
    assert!(renderer.vertex_capacity[renderer.frame] > INITIAL_RING_BYTES);
    // A bind group that had stopped matching its layout would have been
    // refused by the null backend's descriptor check, not merely wrong.
    assert_eq!(
        renderer.constant_buffers.len(),
        FRAMES_IN_FLIGHT,
        "the rebuilt group still names a constants buffer per frame"
    );
    renderer.destroy(device.as_ref());
}

// -----------------------------------------------------------------------
// The image atlas
// -----------------------------------------------------------------------

/// The copy-buffer-to-image commands in `commands`, in order.
fn image_copies(commands: &[crcbl_hal::null::Command]) -> Vec<crcbl_hal::BufferImageCopy> {
    commands
        .iter()
        .filter_map(|command| match command {
            crcbl_hal::null::Command::CopyBufferToImage(copy) => Some(*copy),
            _ => None,
        })
        .collect()
}

/// Records one frame of `ui` through a real graph onto a fresh target and
/// returns the pass labels in execution order.
///
/// The recorder's command stream is filled once the encoder is finished,
/// which this does before returning.
fn record_frame(
    ui: &mut UiRenderer,
    device: &dyn Device,
    queue: QueueHandle,
    pool: &mut crate::transient::TransientPool,
    list: &DrawList,
) -> Vec<String> {
    use crate::graph::{CompiledPass, RenderGraph};
    use crate::transient::TransientImageDesc;
    use crcbl_hal::{CommandEncoderDesc, ImageUsage};

    const EXTENT: (u32, u32) = (64, 48);
    ui.begin_frame(device, list, &FontAtlas::built_in(), 1.0)
        .expect("upload");
    let mut graph = RenderGraph::new(queue);
    let target = graph.create_image(
        "target",
        TransientImageDesc::new(EXTENT, Format::Bgra8UnormSrgb, ImageUsage::COLOR_ATTACHMENT),
    );
    ui.add_passes(&mut graph, target, EXTENT);
    let compiled = graph.compile(pool).expect("a legal frame");
    let labels = compiled
        .passes()
        .iter()
        .map(|pass| CompiledPass::label(pass).to_owned())
        .collect();
    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("ui frame"),
        queue,
    });
    compiled
        .execute(device, pool, encoder.as_mut(), None)
        .expect("the graph executed");
    let commands = encoder.finish().expect("recording succeeded");
    device.destroy_command_buffer(commands);
    labels
}

/// A list with one quad on it, so the draw passes have something to do.
fn one_rect() -> DrawList {
    let mut list = DrawList::new();
    list.rect(glam::Vec2::ZERO, glam::Vec2::splat(8.0), [1.0; 4]);
    list
}

/// Every buffer write the recorder saw, as `(buffer, bytes)`, in order.
fn buffer_writes(recorder: &crcbl_hal::null::Recorder) -> Vec<(BufferHandle, usize)> {
    recorder
        .events()
        .into_iter()
        .filter_map(|event| match event {
            crcbl_hal::null::Event::BufferWritten { buffer, len, .. } => Some((buffer, len)),
            _ => None,
        })
        .collect()
}

/// The rectangle the menu art dirties on an empty page: what start-up has to
/// put on the image atlas and nothing more.
fn menu_art_rect() -> crcbl_ui::image::TexelRect {
    let mut images = ImageAtlas::new();
    crate::menu::menu_skin(&mut images).expect("the menu art fits an empty page");
    images
        .dirty()
        .expect("registering the menu art dirtied the page")
}

/// Whether `buffer` is zeroed across at least `bytes` by a recorded clear
/// that comes before the first command naming it as a copy source.
fn cleared_before_copied(
    commands: &[crcbl_hal::null::Command],
    buffer: BufferHandle,
    bytes: u64,
) -> bool {
    use crcbl_hal::null::Command;

    let cleared = commands.iter().position(|command| {
        matches!(command, Command::ClearBuffer { buffer: cleared, offset: 0, size }
            if *cleared == buffer && *size >= bytes)
    });
    let copied = commands.iter().position(
        |command| matches!(command, Command::CopyBufferToImage(copy) if copy.buffer == buffer),
    );
    matches!((cleared, copied), (Some(cleared), Some(copied)) if cleared < copied)
}

/// **The page is created whole at start-up, zeroed on the GPU, and only the
/// menu art crosses from the host** — as four bytes a texel, into the
/// rectangle it was registered at.
#[test]
fn the_image_atlas_page_is_zeroed_on_the_gpu_and_only_the_art_is_staged() {
    let (recorder, device, queue) = open_recorded();
    let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let commands = recorder.commands();
    let copies: Vec<_> = image_copies(&commands)
        .into_iter()
        .filter(|copy| copy.image == renderer.image_page.image)
        .collect();
    let rect = menu_art_rect();
    let [.., art] = copies[..] else {
        panic!("expected the zeroed page and then the art, got {copies:?}");
    };
    let whole = copies[0];
    let page_bytes = u64::from(PAGE_SIZE) * u64::from(PAGE_SIZE) * 4;
    assert!(
        cleared_before_copied(&commands, whole.buffer, page_bytes),
        "the zero copies must read a buffer zeroed first, in {commands:?}"
    );
    // **Every texel of the page is written exactly once**: zeroes around
    // the art and the art inside its rectangle, never both — two writes to
    // one texel are a hazard Vulkan's sync validation reports.
    let mut writes = vec![0u8; PAGE_SIZE as usize * PAGE_SIZE as usize];
    for copy in &copies {
        assert!(
            copy.buffer == whole.buffer || copy.buffer == art.buffer,
            "a copy from neither the zeroed buffer nor the art: {copy:?}"
        );
        let Offset3d { x, y, .. } = copy.image_offset;
        for row in 0..copy.image_extent.height {
            let start = (y as u32 + row) * PAGE_SIZE + x as u32;
            for texel in start..start + copy.image_extent.width {
                writes[texel as usize] += 1;
            }
        }
    }
    assert!(
        writes.iter().all(|&count| count == 1),
        "a page texel was written twice or never: {copies:?}"
    );
    assert_ne!(art.buffer, whole.buffer, "the art is staged, not zeroed");

    assert_eq!(art.image_extent, Extent3d::d2(rect.width, rect.height));
    assert_eq!(
        (art.image_offset.x, art.image_offset.y),
        (rect.x as i32, rect.y as i32)
    );
    let writes = buffer_writes(&recorder);
    assert!(
        writes.iter().all(|(buffer, _)| *buffer != whole.buffer),
        "the zeroed buffer is never written from the host: {writes:?}"
    );
    assert_eq!(
        writes
            .iter()
            .filter(|(buffer, _)| *buffer == art.buffer)
            .map(|(_, len)| *len)
            .sum::<usize>(),
        rect.width as usize * rect.height as usize * 4,
        "the art's staging is its own rectangle at four bytes a texel"
    );
    assert_eq!(renderer.images().dirty(), None, "the page owes nothing");
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **Construction writes nothing the GPU could zero itself**: the built-in
/// font's bitmap and the menu art's rectangle, and not a byte of either
/// empty page.
///
/// What it guards is the wasm heap. On `crcbl-webgpu` every byte written
/// here is a byte of the start-up frame's command stream, which the heap
/// holds and never gives back — and the whole RGBA image page written from
/// the host, with its padded copies, took shard's peak heap from 18.4 MiB to
/// 41.0 MiB when the image atlas landed, over the browser gate's
/// `WASM_HEAP_CEILING`.
#[test]
fn construction_writes_only_the_font_bitmap_and_the_menu_art() {
    let (recorder, device, queue) = open_recorded();
    let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");

    let (_, _, font) = FontAtlas::built_in().glyph_bitmap();
    let rect = menu_art_rect();
    let art = rect.width as usize * rect.height as usize * 4;
    let writes = buffer_writes(&recorder);
    assert_eq!(
        writes.iter().map(|(_, len)| *len).sum::<usize>(),
        font.len() + art,
        "construction wrote {writes:?}; the font bitmap is {} bytes and the menu art {art}",
        font.len()
    );
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **A frame with nothing registered copies nothing**: no staging, no pass,
/// no import — the steady state every frame after start-up is in.
#[test]
fn a_frame_with_no_new_image_uploads_nothing() {
    let (recorder, device, queue) = open_recorded();
    let mut pool = crate::transient::TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    recorder.clear();

    let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
    assert_eq!(labels, ["ui-composite"]);
    assert!(image_copies(&recorder.commands()).is_empty());

    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **An image registered after start-up is copied once, as exactly the
/// rectangle it dirtied, inside the graph** — ahead of the draws, into the
/// page the graph moved to `TransferDst` and back — and the frame after it
/// copies nothing.
#[test]
fn a_registered_image_is_copied_once_as_its_own_rectangle_inside_the_graph() {
    use crcbl_hal::null::Command;

    let (recorder, device, queue) = open_recorded();
    let mut pool = crate::transient::TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let image = ui
        .images_mut()
        .register(5, 3, &[200; 5 * 3 * 4])
        .expect("fits an empty page");
    let dirty = ui
        .images()
        .dirty()
        .expect("the registration dirtied the page");
    recorder.clear();

    let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
    assert_eq!(
        labels,
        ["ui-images", "ui-composite"],
        "the copy comes before the draw that samples it"
    );
    let commands = recorder.commands();
    let copies = image_copies(&commands);
    assert_eq!(copies.len(), 1, "{copies:?}");
    let copy = copies[0];
    assert_eq!(copy.image, ui.image_page.image);
    assert_eq!(
        (copy.image_offset.x, copy.image_offset.y),
        (dirty.x as i32, dirty.y as i32)
    );
    assert_eq!(copy.image_extent, Extent3d::d2(dirty.width, dirty.height));
    assert!(
        dirty.x <= image.x
            && dirty.y <= image.y
            && image.x + image.width <= dirty.x + dirty.width
            && image.y + image.height <= dirty.y + dirty.height,
        "the copied rectangle {dirty:?} does not cover the image {image:?}"
    );
    // The graph, not the renderer, moved the page out of `ShaderRead` for
    // the copy and back before the draw.
    let page_transitions: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Barrier { images, .. } => Some(images.clone()),
            _ => None,
        })
        .flatten()
        .filter(|barrier| barrier.image == ui.image_page.image)
        .map(|barrier| (barrier.from, barrier.to))
        .collect();
    assert_eq!(
        page_transitions,
        [
            (ResourceState::ShaderRead, ResourceState::TransferDst),
            (ResourceState::TransferDst, ResourceState::ShaderRead),
        ]
    );
    // And back *before* the draw that samples it, not at the end of the
    // frame: a draw reading a page still in `TransferDst` is the hazard the
    // pass's `read_image` declaration exists to rule out.
    let returned = commands
        .iter()
        .position(|command| match command {
            Command::Barrier { images, .. } => images.iter().any(|barrier| {
                barrier.image == ui.image_page.image && barrier.to == ResourceState::ShaderRead
            }),
            _ => false,
        })
        .expect("the page is returned to ShaderRead");
    let drawn = commands
        .iter()
        .position(|command| matches!(command, Command::DrawIndexed { .. }))
        .expect("the rect is drawn");
    assert!(
        returned < drawn,
        "the page went back to ShaderRead at command {returned}, after the draw at {drawn}"
    );

    recorder.clear();
    let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
    assert_eq!(labels, ["ui-composite"], "uploaded once, not every frame");
    assert!(image_copies(&recorder.commands()).is_empty());

    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
    recorder.assert_valid();
}

/// A staged upload whose frame never recorded it is not lost: the next
/// frame stages the same rectangle again.
#[test]
fn an_upload_that_was_never_recorded_is_staged_again() {
    let (device, queue) = open();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    ui.images_mut()
        .register(4, 4, &[9; 4 * 4 * 4])
        .expect("fits");
    let atlas = FontAtlas::built_in();

    ui.begin_frame(device.as_ref(), &one_rect(), &atlas, 1.0)
        .expect("upload");
    let first = ui.image_upload.expect("staged").rect;
    // No graph this frame.
    ui.begin_frame(device.as_ref(), &one_rect(), &atlas, 1.0)
        .expect("upload");
    let again = ui.image_upload.expect("staged again").rect;
    assert_eq!(first, again);

    ui.destroy(device.as_ref());
}

// -----------------------------------------------------------------------
// The glyph pages
// -----------------------------------------------------------------------

/// A list drawing `text` in the committed font at 20px from (4, 4).
fn glyph_run(text: &str) -> DrawList {
    use crcbl_ui::font::Font;
    use crcbl_ui::font::layout::TextLayout;

    let font = Font::sans();
    let layout = TextLayout::new(font, text, 20.0, 24.0, None);
    let mut list = DrawList::new();
    list.glyphs(
        glam::Vec2::splat(4.0),
        font,
        20.0,
        [1.0; 4],
        layout.glyphs(),
    );
    list
}

/// **Every page the atlas may open goes up empty at start-up**, one R8 copy
/// per layer of one image, each from a buffer zeroed on the GPU rather than
/// written from the host.
#[test]
fn the_glyph_pages_are_every_layer_uploaded_empty_at_start_up() {
    let (recorder, device, queue) = open_recorded();
    let renderer = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let commands = recorder.commands();
    let copies: Vec<_> = image_copies(&commands)
        .into_iter()
        .filter(|copy| copy.image == renderer.glyph_pages.image)
        .collect();
    let layers: Vec<u32> = copies
        .iter()
        .map(|copy| {
            assert_eq!(
                copy.image_extent,
                Extent3d::d2(GLYPH_PAGE_SIZE, GLYPH_PAGE_SIZE)
            );
            assert_eq!(copy.image_subresource.layer_count, 1);
            copy.image_subresource.base_layer
        })
        .collect();
    assert_eq!(layers, (0..GLYPH_MAX_PAGES as u32).collect::<Vec<_>>());
    let page_bytes = u64::from(GLYPH_PAGE_SIZE) * u64::from(GLYPH_PAGE_SIZE);
    let writes = buffer_writes(&recorder);
    for copy in &copies {
        assert!(
            cleared_before_copied(&commands, copy.buffer, page_bytes),
            "layer {} must be copied from a buffer zeroed first, in {commands:?}",
            copy.image_subresource.base_layer
        );
        assert!(
            writes.iter().all(|(buffer, _)| *buffer != copy.buffer),
            "the zeroed buffer is never written from the host: {writes:?}"
        );
    }
    renderer.destroy(device.as_ref());
    recorder.assert_valid();
}

/// **A frame that rasterises glyphs copies each dirty page rectangle into
/// its layer, in a `ui-glyphs` pass ahead of the draw** — covering every
/// glyph the frame drew — and the frame after it, drawing the same text,
/// rasterises and copies nothing.
#[test]
fn rasterised_glyphs_are_copied_into_their_page_once_ahead_of_the_draw() {
    use crcbl_hal::null::Command;

    let (recorder, device, queue) = open_recorded();
    let mut pool = crate::transient::TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    recorder.clear();

    let list = glyph_run("Kerned AVATAR");
    let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &list);
    assert_eq!(labels, ["ui-glyphs", "ui-composite"]);
    assert!(ui.glyphs().stats().rasterized > 0);
    let commands = recorder.commands();
    let copies: Vec<_> = image_copies(&commands)
        .into_iter()
        .filter(|copy| copy.image == ui.glyph_pages.image)
        .collect();
    assert_eq!(copies.len(), 1, "one page, one rectangle: {copies:?}");
    let copy = copies[0];
    assert_eq!(copy.image_subresource.base_layer, 0);

    // Every quad the frame drew samples inside the copied rectangle.
    let (vertices, _) = list.to_triangles(None, None, 1.0);
    assert!(vertices.is_empty(), "the run needs the glyph atlas to draw");
    let triangles = ui.last_index_count[ui.frame] / 6;
    assert!(triangles >= 11, "{triangles} glyph quads");
    let page = GLYPH_PAGE_SIZE as f32;
    let mut atlas = crcbl_ui::font::atlas::GlyphAtlas::new(GLYPH_PAGE_SIZE, GLYPH_MAX_PAGES, 1000);
    atlas.begin_frame();
    let (vertices, _) = list.to_triangles(None, Some(&mut atlas), 1.0);
    for vertex in vertices {
        let texel = vertex.uv * page;
        assert!(
            texel.x >= copy.image_offset.x as f32
                && texel.y >= copy.image_offset.y as f32
                && texel.x <= (copy.image_offset.x as u32 + copy.image_extent.width) as f32
                && texel.y <= (copy.image_offset.y as u32 + copy.image_extent.height) as f32,
            "a glyph samples {texel} outside the copied {copy:?}"
        );
    }
    // Back to `ShaderRead` before the draw that samples the pages.
    let returned = commands
        .iter()
        .position(|command| match command {
            Command::Barrier { images, .. } => images.iter().any(|barrier| {
                barrier.image == ui.glyph_pages.image && barrier.to == ResourceState::ShaderRead
            }),
            _ => false,
        })
        .expect("the pages are returned to ShaderRead");
    let drawn = commands
        .iter()
        .position(|command| matches!(command, Command::DrawIndexed { .. }))
        .expect("the glyphs are drawn");
    assert!(returned < drawn);

    recorder.clear();
    let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &list);
    assert_eq!(
        labels,
        ["ui-composite"],
        "cached glyphs were uploaded again"
    );
    assert_eq!(ui.glyphs().stats().rasterized, 0);
    assert!(image_copies(&recorder.commands()).is_empty());

    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
    recorder.assert_valid();
}

/// A staged page copy whose frame never recorded it is staged again.
#[test]
fn a_glyph_upload_that_was_never_recorded_is_staged_again() {
    let (device, queue) = open();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    let atlas = FontAtlas::built_in();
    let list = glyph_run("again");
    ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
        .expect("upload");
    let first: Vec<TexelRect> = ui.glyph_uploads.iter().map(|upload| upload.rect).collect();
    assert_eq!(first.len(), 1);
    // No graph this frame; the glyphs are cached, so only the retry stages.
    ui.begin_frame(device.as_ref(), &list, &atlas, 1.0)
        .expect("upload");
    let again: Vec<TexelRect> = ui.glyph_uploads.iter().map(|upload| upload.rect).collect();
    assert_eq!(first, again);
    ui.destroy(device.as_ref());
}

/// Every glyph staging buffer is given back, by the ring or by `destroy`.
#[test]
fn glyph_uploads_leak_nothing() {
    let (recorder, device, queue) = open_recorded();
    let before = recorder.total_live_objects();
    let mut pool = crate::transient::TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    for (round, text) in ["abc", "def", "ghi", "jkl"].into_iter().enumerate() {
        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &glyph_run(text));
        assert_eq!(labels[0], "ui-glyphs", "round {round}");
    }
    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
    assert_eq!(recorder.total_live_objects(), before);
    recorder.assert_valid();
}

/// Every staging buffer an upload made is given back — by the ring on its
/// next turn, or by `destroy` for the ones still in flight.
#[test]
fn image_uploads_leak_nothing() {
    let (recorder, device, queue) = open_recorded();
    let before = recorder.total_live_objects();
    let mut pool = crate::transient::TransientPool::new();
    let mut ui = UiRenderer::new(device.as_ref(), queue, Format::Bgra8UnormSrgb).expect("built");
    for round in 0..(FRAMES_IN_FLIGHT * 2) {
        ui.images_mut()
            .register(3, 3, &[round as u8; 3 * 3 * 4])
            .expect("fits");
        let labels = record_frame(&mut ui, device.as_ref(), queue, &mut pool, &one_rect());
        assert_eq!(labels[0], "ui-images", "round {round}");
    }
    ui.destroy(device.as_ref());
    pool.destroy(device.as_ref());
    assert_eq!(recorder.total_live_objects(), before);
    recorder.assert_valid();
}
