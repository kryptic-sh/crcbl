use super::*;

#[test]
fn frame_scratch_keeps_capacity_after_motion_and_settling() {
    let (recorder, device) = open();
    let mut pool = pool(device.as_ref(), 8);
    let handles: Vec<_> = (1..=8)
        .map(|n| pool.insert(&instance(n)).expect("room"))
        .collect();
    settle(&mut pool, device.as_ref());
    for (index, handle) in handles.iter().enumerate() {
        assert!(pool.set(*handle, &instance(index as u32 * 2 + 17)));
    }
    pool.rotate();
    pool.flush(device.as_ref()).expect("upload");
    let carry_capacity = pool.written_last_frame.capacity();
    let dirty_capacity = pool.dirty[pool.frame].runs.capacity();
    assert!(carry_capacity >= handles.len());
    assert!(
        dirty_capacity > 0,
        "successful flush retains its run storage"
    );
    for _ in 0..FRAMES * 3 {
        pool.rotate();
        pool.flush(device.as_ref()).expect("upload");
        assert!(pool.written_this_frame.is_empty());
        assert!(pool.written_last_frame.is_empty());
        assert!(
            pool.written_this_frame.capacity() >= carry_capacity
                || pool.written_last_frame.capacity() >= carry_capacity
        );
        assert!(pool.dirty[pool.frame].runs.capacity() >= dirty_capacity);
        for (index, _) in handles.iter().enumerate() {
            let n = index as u32 * 2 + 17;
            assert_eq!(
                on_device(&recorder, &pool, pool.frame, index as u32),
                stored(n)
            );
        }
    }
    pool.destroy(device.as_ref());
    recorder.assert_valid();
    assert_eq!(recorder.total_live_objects(), 0);
}

#[test]
fn failed_upload_keeps_committed_prefix_and_retries_dirty_suffix() {
    use crcbl_hal::null::{Event, NullInstance, Recorder};
    use crcbl_hal::{AdapterId, DeviceDesc, Instance as _};
    for fail_at in [0usize, 1, 2] {
        let rec = Recorder::new();
        let backend = NullInstance::gpu_driven().with_recorder(rec.clone());
        let device = backend
            .create_device(&DeviceDesc::for_adapter(AdapterId(0)))
            .unwrap();
        let mut pool = InstancePool::new(
            device.as_ref(),
            &InstancePoolDesc {
                label: Some("retry fixture"),
                capacity: 6,
                frames_in_flight: 3,
            },
        )
        .unwrap();
        let handles: Vec<_> = (0..6)
            .map(|i| {
                pool.insert(&GpuInstance {
                    mesh: i,
                    material: 7,
                    ..GpuInstance::default()
                })
                .unwrap()
            })
            .collect();
        for _ in 0..3 {
            pool.begin_frame(device.as_ref()).unwrap();
        }
        let slot = pool.frame;
        for i in [0usize, 2, 4] {
            let mut row = pool.get(handles[i]).unwrap();
            row.material = 40 + i as u32;
            assert!(pool.set(handles[i], &row));
        }
        let expected_runs = vec![0..1, 2..3, 4..5];
        assert_eq!(pool.dirty[slot].runs, expected_runs);
        let capacity = pool.dirty[slot].runs.capacity();
        let size = (fail_at * 2 * INSTANCE_STRIDE) as u64;
        let short = device
            .create_buffer(&BufferDesc {
                label: Some("short retry target"),
                size: size.max(1),
                usage: BufferUsage::STORAGE,
                memory: MemoryLocation::HostUpload,
            })
            .unwrap();
        let original = pool.buffers[slot];
        pool.buffers[slot] = short;
        rec.clear();
        assert!(matches!(
            pool.flush(device.as_ref()),
            Err(HalError::InvalidDescriptor(_))
        ));
        let writes: Vec<_> = rec
            .events()
            .into_iter()
            .filter_map(|e| match e {
                Event::BufferWritten {
                    buffer,
                    offset,
                    len,
                } => Some((buffer, offset, len)),
                _ => None,
            })
            .collect();
        let expected: Vec<_> = expected_runs[..fail_at]
            .iter()
            .map(|r| {
                (
                    short,
                    r.start as u64 * INSTANCE_STRIDE as u64,
                    INSTANCE_STRIDE,
                )
            })
            .collect();
        assert_eq!(writes, expected);
        let short_bytes = rec.buffer_bytes(short).unwrap();
        for r in &expected_runs[..fail_at] {
            let at = r.start as usize * INSTANCE_STRIDE;
            assert_eq!(
                &short_bytes[at..at + INSTANCE_STRIDE],
                &pool.mirror[at..at + INSTANCE_STRIDE]
            );
            device
                .write_buffer(original, at as u64, &short_bytes[at..at + INSTANCE_STRIDE])
                .unwrap();
        }
        assert_eq!(pool.dirty[slot].runs, expected_runs[fail_at..]);
        assert_eq!(pool.dirty[slot].runs.capacity(), capacity);
        let mirror = pool.mirror.clone();
        pool.buffers[slot] = original;
        rec.clear();
        pool.flush(device.as_ref()).unwrap();
        let writes: Vec<_> = rec
            .events()
            .into_iter()
            .filter_map(|e| match e {
                Event::BufferWritten {
                    buffer,
                    offset,
                    len,
                } => Some((buffer, offset, len)),
                _ => None,
            })
            .collect();
        let expected: Vec<_> = expected_runs[fail_at..]
            .iter()
            .map(|r| {
                (
                    original,
                    r.start as u64 * INSTANCE_STRIDE as u64,
                    INSTANCE_STRIDE,
                )
            })
            .collect();
        assert_eq!(writes, expected);
        assert_eq!(rec.buffer_bytes(original).unwrap(), mirror);
        assert!(pool.dirty[slot].runs.is_empty());
        assert_eq!(pool.dirty[slot].runs.capacity(), capacity);
        rec.clear();
        pool.flush(device.as_ref()).unwrap();
        assert!(
            !rec.events()
                .iter()
                .any(|e| matches!(e, Event::BufferWritten { .. }))
        );
        for other in 0..pool.buffers.len() {
            if other != slot {
                assert_eq!(pool.dirty[other].runs, expected_runs);
            }
        }
        device.destroy_buffer(short);
        pool.destroy(device.as_ref());
        rec.assert_valid();
        assert_eq!(rec.total_live_objects(), 0);
    }
}
