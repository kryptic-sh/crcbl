use super::*;
use crcbl_shaders::skinning::{JOINT_STRIDE, PARAMS_SIZE, Params, SKIN_BINDING_STRIDE};

#[test]
fn skinned_instance_refusal_preserves_prior_skinning_and_retries_pending_uploads() {
    for fail_at in [0usize, 1, 2] {
        let (recorder, device, queue) = open();
        let device = device.as_ref();
        let mut fixture = SkinnedFixture::build(device, queue);
        let skinned = fixture
            .renderer
            .add_skinned_instance(&SkinnedInstanceDesc {
                mesh: &fixture.skinned,
                material: DEMO_UNTINTED,
                transform: Mat4::IDENTITY,
            })
            .unwrap();
        let handles: Vec<_> = (0..4)
            .map(|index| {
                place_cube(
                    &mut fixture.renderer,
                    Mat4::from_translation(Vec3::X * index as f32),
                )
            })
            .collect();
        let mut skin_targets = vec![Vec::new(); FRAMES_IN_FLIGHT];
        for _ in 0..FRAMES_IN_FLIGHT + 1 {
            recorder.clear();
            fixture.begin(device);
            skin_targets[fixture.renderer.frame] = buffer_writes(&recorder)[..3]
                .iter()
                .map(|(buffer, _, _)| *buffer)
                .collect();
        }
        let indices = [
            skinned.index() as usize,
            handles[1].index() as usize,
            handles[3].index() as usize,
        ];
        assert!(indices.windows(2).all(|pair| pair[1] > pair[0] + 1));
        for index in [1usize, 3] {
            fixture.renderer.set_instance(
                handles[index],
                &InstanceDesc {
                    mesh: DEMO_CUBE,
                    material: DEMO_UNTINTED,
                    transform: Mat4::from_translation(Vec3::Y * (index + 1) as f32),
                },
            );
        }
        fixture.palette[0] = Mat4::from_translation(Vec3::Z * 7.0);
        let slot = (fixture.renderer.frame + 1) % FRAMES_IN_FLIGHT;
        let original = fixture.renderer.instances.buffers()[slot];
        let before = recorder.buffer_bytes(original).unwrap();
        let size = (indices[fail_at] * INSTANCE_STRIDE).max(1);
        let short = device
            .create_buffer(&BufferDesc {
                label: Some("refused skinned instance upload"),
                size: size as u64,
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .unwrap();
        device.write_buffer(short, 0, &before[..size]).unwrap();
        assert_eq!(
            fixture.renderer.instances.replace_test_buffer(slot, short),
            original
        );
        let effects = fixture.renderer.frame_effects;
        let mut request = fixture.renderer.effect_request();
        request.programmatic = request.programmatic.force(
            RenderEffects::BLOOM,
            Some(!effects.contains(RenderEffects::BLOOM)),
        );
        fixture.renderer.set_effect_request(request);
        assert_ne!(fixture.renderer.resolved_effects(), effects);
        let shadow_inputs = fixture.renderer.shadow_group_inputs.clone();
        let old_parity = fixture.skinning.parity();
        recorder.clear();
        let range = fixture
            .skinned
            .skin_range(&fixture.palette, &fixture.bindings);
        let result = fixture.renderer.begin_skinned_frame(
            device,
            &mut fixture.skinning,
            &[range],
            &Camera::default(),
            &DirectionalLight::default(),
            TEST_EXTENT,
        );
        assert_eq!(
            fixture
                .renderer
                .instances
                .replace_test_buffer(slot, original),
            short
        );
        assert!(
            matches!(result, Err(SkinningError::Hal(HalError::InvalidDescriptor(ref cause))) if cause.contains("exceeds")),
            "{result:?}"
        );
        assert_eq!(fixture.renderer.frame, slot);
        assert!(fixture.renderer.frame_skins);
        let parity = fixture.skinning.parity();
        assert_ne!(
            parity, old_parity,
            "accepted skinning precedes the instance refusal"
        );
        assert_eq!(fixture.renderer.frame_effects, effects);
        assert_eq!(fixture.renderer.shadow_group_inputs, shadow_inputs);
        let pointed = fixture.renderer.instances.get(skinned).unwrap();
        assert_eq!(pointed.base_vertex, fixture.skinned.region().base(parity));
        assert_eq!(
            pointed.previous_base_vertex,
            fixture.skinned.region().previous_base(parity)
        );
        let expected = uploaded_bytes(&fixture.renderer, before.len());
        let targets = &skin_targets[slot];
        let mut expected_writes = vec![
            (targets[0], 0, PARAMS_SIZE),
            (targets[1], 0, fixture.palette.len() * JOINT_STRIDE),
            (targets[2], 0, fixture.bindings.len() * SKIN_BINDING_STRIDE),
        ];
        expected_writes.extend(
            indices[..fail_at]
                .iter()
                .map(|index| (short, (*index * INSTANCE_STRIDE) as u64, INSTANCE_STRIDE)),
        );
        assert_eq!(
            buffer_writes(&recorder),
            expected_writes,
            "only skinning and committed instance writes precede refusal"
        );
        let params = Params {
            vertex_count: fixture.skinned.vertex_count(),
            input_base: fixture.skinned.input_base(),
            output_base: fixture.skinned.region().base(parity),
            binding_base: 0,
            joint_base: 0,
            joint_count: fixture.palette.len() as u32,
            attribute_base: fixture.renderer.attribute_base(),
        };
        assert_eq!(
            recorder.buffer_bytes(targets[0]).unwrap(),
            params.to_bytes()
        );
        let palette_bytes: Vec<_> = fixture
            .palette
            .iter()
            .flat_map(|matrix| {
                matrix
                    .to_cols_array()
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
            })
            .collect();
        assert_eq!(recorder.buffer_bytes(targets[1]).unwrap(), palette_bytes);
        let binding_bytes: Vec<_> = fixture
            .bindings
            .iter()
            .flat_map(|binding| binding.to_bytes())
            .collect();
        assert_eq!(recorder.buffer_bytes(targets[2]).unwrap(), binding_bytes);
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
        fixture.renderer.instances.flush(device).unwrap();
        let suffix: Vec<_> = indices[fail_at..]
            .iter()
            .map(|index| (original, (*index * INSTANCE_STRIDE) as u64, INSTANCE_STRIDE))
            .collect();
        assert_eq!(buffer_writes(&recorder), suffix);
        assert_eq!(recorder.buffer_bytes(original).unwrap(), expected);
        recorder.clear();
        fixture.renderer.instances.flush(device).unwrap();
        assert!(recorder.events().is_empty());
        for _ in 0..FRAMES_IN_FLIGHT * 2 {
            fixture.begin(device);
            let buffer = fixture.renderer.instances.buffers()[fixture.renderer.frame];
            assert_eq!(
                recorder.buffer_bytes(buffer).unwrap(),
                uploaded_bytes(&fixture.renderer, before.len())
            );
            let record = instance_record(&recorder, &fixture.renderer, skinned);
            let parity = fixture.skinning.parity();
            assert_eq!(record.base_vertex, fixture.skinned.region().base(parity));
            assert_eq!(
                record.previous_base_vertex,
                fixture.skinned.region().previous_base(parity)
            );
        }
        device.destroy_buffer(short);
        fixture.destroy(device);
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }
}
