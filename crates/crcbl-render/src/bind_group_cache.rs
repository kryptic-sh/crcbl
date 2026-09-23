//! Bind groups cached against the image views a render graph realizes.

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutHandle, BindingResource, Device,
    ImageViewHandle,
};

/// A bind group naming each `(binding, view)` of `views`, built once and kept
/// until any of those views changes.
///
/// **The shape every group in this engine that names a graph transient has.** A
/// transient's view is realised by the graph and is therefore not known until a
/// pass body runs, so such a group cannot be built where the others are — and
/// rebuilding it every frame would be a descriptor write per frame for handles
/// that only ever change on a resize. `entries` is the group's complete
/// description with any value at each of those bindings; this replaces those
/// entries.
///
/// A group naming one transient passes a one-element slice. The cache key is
/// **every** view, because a group naming two of them is stale as soon as either
/// moves — which is what the blur pass needs and what a key on the first alone
/// would silently get wrong on a resize.
///
/// [`None`] means the group could not be created and the caller should record
/// nothing. Recording a pass that draws nothing is better than aborting a frame:
/// the window loses that pass's contribution, the log says why, and the next
/// frame retries.
///
/// **On a backend whose creation cannot fail synchronously this returns [`Some`]
/// regardless**, and that is deliberate rather than a gap. A command-stream
/// backend hands back a handle it allocated itself and learns the browser's
/// verdict later, so the pass records its draw, the invalid group makes the
/// submission invalid, and the failure arrives through
/// [`Device::take_error`] — where
/// `crcbl::engine`'s frame acquire turns it into an error that stops the frame.
/// That is louder than skipping a pass, and it is the right way round: a bind
/// group this code built wrongly is a bug, not a device that ran out of room.
/// The branch below stays for the backends that can still answer immediately.
///
/// # Panics
///
/// If `entries` has no entry at one of the bindings — which is a caller that
/// built its list against a different layout than the one it passed.
pub(crate) fn cached_group(
    cache: &mut Option<(Vec<ImageViewHandle>, BindGroupHandle)>,
    device: &dyn Device,
    views: &[(u32, ImageViewHandle)],
    label: &str,
    layout: BindGroupLayoutHandle,
    entries: &[BindGroupEntry],
) -> Option<BindGroupHandle> {
    if let Some((cached, group)) = cache
        && cached.iter().eq(views.iter().map(|(_, view)| view))
    {
        return Some(*group);
    }
    if let Some((_, stale)) = cache.take() {
        device.destroy_bind_group(stale);
    }
    let mut entries = entries.to_vec();
    for &(binding, view) in views {
        let slot = entries
            .iter_mut()
            .find(|entry| entry.binding == binding)
            .unwrap_or_else(|| panic!("{label}: the entry list has no binding {binding} to fill"));
        slot.resource = BindingResource::ImageView(view);
    }
    match device.create_bind_group(&BindGroupDesc {
        label: Some(label),
        layout,
        entries: &entries,
        variable_count: None,
    }) {
        Ok(group) => {
            *cache = Some((views.iter().map(|(_, view)| *view).collect(), group));
            Some(group)
        }
        Err(error) => {
            crcbl_core::log::error!("graph: {label} bind group failed: {error}");
            None
        }
    }
}

// `Instance::create_device` is native-only: see the `crcbl_hal::device` module docs.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crcbl_hal::null::{Event, NullInstance, ObjectKind, Recorder};
    use crcbl_hal::*;

    #[test]
    fn all_views_invalidate_and_failed_creation_retries() {
        let recorder = Recorder::new();
        let instance = NullInstance::gpu_driven().with_recorder(recorder.clone());
        let adapter = instance.adapters().remove(0);
        let device = instance
            .create_device(&DeviceDesc::for_adapter(adapter.id))
            .unwrap();
        let images: Vec<_> = (0..3)
            .map(|_| {
                device
                    .create_image(&ImageDesc {
                        label: Some("cache probe"),
                        image_type: ImageType::D2,
                        extent: Extent3d::d2(1, 1),
                        format: Format::Rgba8Unorm,
                        mip_levels: 1,
                        samples: 1,
                        usage: ImageUsage::SAMPLED,
                    })
                    .unwrap()
            })
            .collect();
        let views: Vec<_> = images
            .iter()
            .map(|&image| {
                device
                    .create_image_view(&ImageViewDesc {
                        label: Some("cache probe view"),
                        image,
                        view_type: ImageViewType::D2,
                        format: Format::Rgba8Unorm,
                        range: ImageSubresourceRange::all(Format::Rgba8Unorm),
                    })
                    .unwrap()
            })
            .collect();
        let slots = [0, 1].map(|binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            kind: BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::UnfilterableFloat,
            },
            count: 1,
            flags: BindingFlags::empty(),
        });
        let layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("cache probe"),
                entries: &slots,
            })
            .unwrap();
        let bad_layout = device
            .create_bind_group_layout(&BindGroupLayoutDesc {
                label: Some("missing second slot"),
                entries: &slots[..1],
            })
            .unwrap();
        let template = [0, 1].map(|binding| BindGroupEntry {
            binding,
            array_index: 0,
            resource: BindingResource::ImageView(views[0]),
        });
        let original = template;
        let mut cache = None;
        let call = |cache: &mut Option<(Vec<ImageViewHandle>, BindGroupHandle)>,
                    key: &[(u32, ImageViewHandle)],
                    layout| {
            cached_group(
                cache,
                device.as_ref(),
                key,
                "cache probe",
                layout,
                &template,
            )
        };
        let first_key = [(0, views[0]), (1, views[1])];
        let first = call(&mut cache, &first_key, layout).unwrap();
        assert_eq!(recorder.live_objects(ObjectKind::BindGroup), 1);
        assert_eq!(call(&mut cache, &first_key, layout), Some(first));
        let second_key = [(0, views[0]), (1, views[2])];
        let second = call(&mut cache, &second_key, layout).unwrap();
        assert_ne!(first, second, "changing only the second view must rebuild");
        assert_eq!(recorder.live_objects(ObjectKind::BindGroup), 1);
        assert_eq!(cache.as_ref().unwrap().0, vec![views[0], views[2]]);
        assert_eq!(call(&mut cache, &first_key, bad_layout), None);
        assert_eq!(cache, None, "failed replacement must retry next time");
        assert_eq!(recorder.live_objects(ObjectKind::BindGroup), 0);
        let retry = call(&mut cache, &first_key, layout).unwrap();
        assert_ne!(retry, second);
        assert_eq!(template, original, "borrowed template must stay unchanged");
        recorder.assert_valid();
        recorder.clear();
        assert_eq!(call(&mut cache, &first_key, layout), Some(retry));
        assert!(!recorder.events().iter().any(|event| matches!(
            event,
            Event::Created {
                kind: ObjectKind::BindGroup,
                ..
            }
        )));
        device.destroy_bind_group(cache.take().unwrap().1);
        device.destroy_bind_group_layout(layout);
        device.destroy_bind_group_layout(bad_layout);
        for view in views {
            device.destroy_image_view(view);
        }
        for image in images {
            device.destroy_image(image);
        }
        recorder.assert_valid();
        assert_eq!(recorder.total_live_objects(), 0);
    }
}
