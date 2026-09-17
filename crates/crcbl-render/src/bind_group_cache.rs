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
    mut entries: Vec<BindGroupEntry>,
) -> Option<BindGroupHandle> {
    if let Some((cached, group)) = cache
        && cached.iter().eq(views.iter().map(|(_, view)| view))
    {
        return Some(*group);
    }
    if let Some((_, stale)) = cache.take() {
        device.destroy_bind_group(stale);
    }
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
