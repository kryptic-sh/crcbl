mod skinned;

use super::*;
use crcbl_shaders::mesh::{GpuInstance, INSTANCE_STRIDE};

fn buffer_writes(recorder: &Recorder) -> Vec<(BufferHandle, u64, usize)> {
    recorder
        .events()
        .into_iter()
        .filter_map(|event| match event {
            Event::BufferWritten {
                buffer,
                offset,
                len,
            } => Some((buffer, offset, len)),
            _ => None,
        })
        .collect()
}

fn uploaded_bytes(renderer: &ForwardRenderer, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    for (index, record) in renderer.cull_records().0.iter().enumerate() {
        let at = index * INSTANCE_STRIDE;
        bytes[at..at + INSTANCE_STRIDE].copy_from_slice(&record.to_bytes());
    }
    bytes
}

#[test]
fn refused_instance_upload_stops_preparation_and_recovers_across_the_ring() {
    for fail_at in [0usize, 1, 2] {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let (mut renderer, _) = shadow_cache_scene(device, queue);
        let handles: Vec<_> = (0..6)
            .map(|index| {
                place_cube(
                    &mut renderer,
                    Mat4::from_translation(Vec3::X * index as f32),
                )
            })
            .collect();
        let camera = Camera::default();
        let sun = DirectionalLight::default();
        for _ in 0..FRAMES_IN_FLIGHT + 1 {
            renderer
                .begin_frame(device, &camera, &sun, TEST_EXTENT)
                .unwrap();
        }
        let indices: Vec<_> = [0usize, 2, 4]
            .into_iter()
            .map(|index| {
                let handle = handles[index];
                renderer.set_instance(
                    handle,
                    &InstanceDesc {
                        mesh: DEMO_CUBE,
                        material: DEMO_UNTINTED,
                        transform: Mat4::from_translation(Vec3::Y * (index + 1) as f32),
                    },
                );
                handle.index() as usize
            })
            .collect();
        let slot = (renderer.frame + 1) % FRAMES_IN_FLIGHT;
        let original = renderer.instances.buffers()[slot];
        let before = recorder.buffer_bytes(original).unwrap();
        let expected = uploaded_bytes(&renderer, before.len());
        assert_ne!(
            expected, before,
            "the fixture must have an outstanding upload"
        );
        let size = indices[fail_at] * INSTANCE_STRIDE;
        let short = device
            .create_buffer(&BufferDesc {
                label: Some("refused renderer instance upload"),
                size: size as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .unwrap();
        device.write_buffer(short, 0, &before[..size]).unwrap();
        assert_eq!(
            renderer.instances.replace_test_buffer(slot, short),
            original
        );
        let shadow_inputs = renderer.shadow_group_inputs.clone();
        let effects = renderer.frame_effects;
        let mut request = renderer.effect_request();
        request.programmatic = request.programmatic.force(
            RenderEffects::BLOOM,
            Some(!effects.contains(RenderEffects::BLOOM)),
        );
        renderer.set_effect_request(request);
        assert_ne!(
            renderer.resolved_effects(),
            effects,
            "pending effects must differ"
        );
        recorder.clear();
        let result = renderer.begin_frame(device, &camera, &sun, TEST_EXTENT);
        assert_eq!(
            renderer.instances.replace_test_buffer(slot, original),
            short
        );
        assert!(
            matches!(result, Err(HalError::InvalidDescriptor(ref cause)) if cause.contains("exceeds")),
            "{result:?}"
        );
        assert_eq!(renderer.frame, slot, "rotation precedes the refused upload");
        assert_eq!(renderer.shadow_group_inputs, shadow_inputs);
        assert_eq!(renderer.frame_effects, effects);
        let writes = buffer_writes(&recorder);
        let prefix: Vec<_> = indices[..fail_at]
            .iter()
            .map(|index| (short, (*index * INSTANCE_STRIDE) as u64, INSTANCE_STRIDE))
            .collect();
        assert_eq!(
            writes, prefix,
            "no shared preparation writes follow a refusal"
        );
        let mut expected_short = before[..size].to_vec();
        for index in &indices[..fail_at] {
            let at = *index * INSTANCE_STRIDE;
            expected_short[at..at + INSTANCE_STRIDE]
                .copy_from_slice(&expected[at..at + INSTANCE_STRIDE]);
        }
        let short_bytes = recorder.buffer_bytes(short).unwrap();
        assert_eq!(short_bytes, expected_short);
        // Transfer the committed prefix when restoring the full-size destination.
        for index in &indices[..fail_at] {
            let at = *index * INSTANCE_STRIDE;
            device
                .write_buffer(original, at as u64, &short_bytes[at..at + INSTANCE_STRIDE])
                .unwrap();
        }
        recorder.clear();
        renderer.instances.flush(device).unwrap();
        let writes = buffer_writes(&recorder);
        let suffix: Vec<_> = indices[fail_at..]
            .iter()
            .map(|index| (original, (*index * INSTANCE_STRIDE) as u64, INSTANCE_STRIDE))
            .collect();
        assert_eq!(writes, suffix);
        assert_eq!(recorder.buffer_bytes(original).unwrap(), expected);
        recorder.clear();
        renderer.instances.flush(device).unwrap();
        assert!(recorder.events().is_empty(), "retry is idempotent");
        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            renderer
                .begin_frame(device, &camera, &sun, TEST_EXTENT)
                .unwrap();
            let buffer = renderer.instances.buffers()[renderer.frame];
            assert_eq!(
                recorder.buffer_bytes(buffer).unwrap(),
                uploaded_bytes(&renderer, before.len())
            );
        }
        assert!(
            renderer
                .cull_records()
                .0
                .iter()
                .all(|record: &GpuInstance| record.previous_transform == record.transform)
        );
        device.destroy_buffer(short);
        renderer.destroy(device);
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }
}
