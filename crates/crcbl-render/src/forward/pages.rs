//! The material pages and their sampler as each frame slot binds them, and a
//! page replaced under a running renderer.
//!
//! ```text
//! replace_page ──upload whole──▶ new image ──▶ ForwardRenderer::<kind>_page
//!                                                  │
//! begin_frame (slot k) ──adopt_pages──▶ slot k's groups rebuilt naming it
//!                                                  │
//!                     the last slot moves off ──▶ old image destroyed
//! ```
//!
//! # A replaced page is a new image, never a write into the old one
//!
//! A frame already submitted samples the page its groups name, and a copy into
//! that image while it does is the half-uploaded frame: some texels old, some
//! new, and on a backend with no hazard tracking of its own a read racing a
//! write. So [`ForwardRenderer::replace_page`] uploads the whole page into a new
//! image — every layer and its chain, through the same [`upload_page`] the
//! renderer was built with — and nothing a frame in flight reads is touched.
//! A changed extent therefore costs nothing extra: it is a new allocation
//! either way.
//!
//! # Each slot moves at its own frame, and the old page dies after the last
//!
//! [`PageBindings`] is what one slot's mesh-layout groups name: the sampler and
//! one view per [`PageKind`]. A slot moves onto the renderer's current answer
//! at its own `begin_frame`, which is the moment the ring guarantees the slot's
//! previous submission has retired — so its groups can be destroyed and
//! rebuilt — and a replaced page or sampler is destroyed once no slot names it.
//! That is the retirement [`set_anisotropy`](ForwardRenderer::set_anisotropy)
//! already had for the sampler, widened to the pages: one rule for every
//! resource these groups hold by handle, and one rebuild when several change
//! between two frames of a slot.
//!
//! **Counted in slots, not in frames.** A frame counter would free the old
//! page `FRAMES_IN_FLIGHT` frames later whether or not every slot had moved —
//! and a slot whose rebuild was refused has not, and goes on drawing with the
//! old page until its next `begin_frame` tries again. Retiring on "no slot
//! names it" is the condition itself rather than a count that usually implies
//! it. What happens to the device object after that is the HAL's
//! `destroy_image` contract: the handle is dead at once, and a backend that
//! keeps a deletion queue (`crcbl-vk`, `crcbl-dx12`) holds the free behind the
//! submissions that used it.
//!
//! # Material rows are not touched
//!
//! A replacement keeps the kind's layer count, so every row's layer index still
//! names the same texture in the new image: a material referencing a reloaded
//! texture needs no rewrite. A replacement that would add or drop layers is
//! refused — it changes which layer a row's number means, and that is a new
//! scene rather than a reload.

use super::*;

/// What one frame slot's mesh-layout groups name of the material pages: the
/// sampler at [`PAGE_SAMPLER_BINDING`] and, per [`PageKind`] in
/// [`PageKind::ALL`] order, the view at that kind's binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PageBindings {
    pub(super) sampler: SamplerHandle,
    pub(super) views: [ImageViewHandle; PageKind::ALL.len()],
}

impl PageBindings {
    /// `entries` with the page sampler and every page view rewritten to these.
    ///
    /// # Panics
    ///
    /// If `entries` lacks one of the bindings, which every group of the mesh
    /// layout carries — `label` names the group.
    fn write_into(&self, entries: &mut [BindGroupEntry], label: &str) {
        let mut written = 0;
        for entry in entries.iter_mut() {
            let resource = if entry.binding == PAGE_SAMPLER_BINDING {
                Some(BindingResource::Sampler(self.sampler))
            } else {
                PageKind::ALL
                    .into_iter()
                    .find(|kind| page_binding(*kind) == entry.binding)
                    .map(|kind| BindingResource::ImageView(self.views[kind.index()]))
            };
            if let Some(resource) = resource {
                entry.resource = resource;
                written += 1;
            }
        }
        assert_eq!(
            written,
            1 + PageKind::ALL.len(),
            "{label}: every mesh-layout group names the page sampler and every page"
        );
    }
}

/// The mesh-layout binding `kind`'s page view sits at.
pub(super) const fn page_binding(kind: PageKind) -> u32 {
    match kind {
        PageKind::BaseColor => BASE_COLOR_PAGE_BINDING,
        PageKind::Normal => NORMAL_PAGE_BINDING,
        PageKind::MetallicRoughnessOcclusion => MRO_PAGE_BINDING,
        PageKind::Emissive => EMISSIVE_PAGE_BINDING,
    }
}

/// A sampler or a page some slot's groups may still name, waiting for the last
/// slot to move off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetiredPage {
    /// A page sampler [`set_anisotropy`](ForwardRenderer::set_anisotropy)
    /// replaced.
    Sampler(SamplerHandle),
    /// A page [`replace_page`](ForwardRenderer::replace_page) replaced.
    Page(UploadedTexture),
}

impl RetiredPage {
    /// Whether a slot binding `bindings` still names this.
    fn named_by(&self, bindings: &PageBindings) -> bool {
        match self {
            Self::Sampler(sampler) => bindings.sampler == *sampler,
            Self::Page(page) => bindings.views.contains(&page.view),
        }
    }

    /// Releases it. No slot may name it.
    pub(super) fn destroy(self, device: &dyn Device) {
        match self {
            Self::Sampler(sampler) => device.destroy_sampler(sampler),
            Self::Page(page) => page.destroy(device),
        }
    }
}

/// Uploads one kind's page — `layers` at `extent` texels a side, each with the
/// mip chain its kind's filter builds — and returns the image and the extent it
/// was created at.
///
/// **A kind with no layers takes one texel**: see [`PAGE_PLACEHOLDER_TEXEL`].
/// The layers' lengths against `extent` are the caller's to have checked —
/// `PageDesc::check` — before this is reached.
///
/// `service` is called once per layer chain built, for
/// [`ForwardRenderer::with_scene_serviced`]'s reason: a chain is host work
/// proportional to its texels.
pub(super) fn upload_page(
    device: &dyn Device,
    queue: QueueHandle,
    kind: PageKind,
    extent: u32,
    layers: &[std::borrow::Cow<'_, [u8]>],
    service: &mut dyn FnMut(),
) -> Result<(UploadedTexture, u32), HalError> {
    let chains: Vec<Vec<Vec<u8>>> = layers
        .iter()
        .map(|texels| {
            let chain = kind.chain(texels, extent);
            service();
            chain
        })
        .collect();
    let levels: Vec<Vec<&[u8]>> = layers
        .iter()
        .zip(&chains)
        .map(|(level0, below)| {
            std::iter::once(level0.as_ref())
                .chain(below.iter().map(Vec::as_slice))
                .collect()
        })
        .collect();
    let placeholder: [&[u8]; 1] = [&PAGE_PLACEHOLDER_TEXEL[..]];
    let (extent, uploading): (u32, Vec<&[&[u8]]>) = if levels.is_empty() {
        (1, vec![&placeholder[..]])
    } else {
        (extent, levels.iter().map(Vec::as_slice).collect())
    };
    let page = upload_texture_mip_layers(
        device,
        queue,
        kind.upload_label(),
        kind.format(),
        extent,
        extent,
        &uploading,
    )?;
    Ok((page, extent))
}

impl ForwardRenderer {
    /// Replaces `kind`'s material page with `page`'s layers of that kind, in
    /// force from each frame slot's next [`begin_frame`](Self::begin_frame).
    ///
    /// Texture hot reload's device half: an edited texture is re-decoded, its
    /// page rebuilt on the host, and handed here. The whole page goes up into
    /// a **new image**, because a copy into the image a frame in flight
    /// samples is a frame that reads some texels old and some new — and each
    /// slot's groups move onto it at that slot's own frame, the replaced image
    /// living until the last slot has left it. `page`'s extent for `kind` may differ from the
    /// one in force; its layer count may not, because every material row names
    /// a layer by number and those numbers have to go on meaning the same
    /// textures.
    ///
    /// `page`'s other kinds are not read beyond the length check every layer
    /// gets. A kind the renderer was built with no layers of has nothing to
    /// replace, and replacing it with none is accepted and creates nothing.
    ///
    /// **A startup-path upload**, as [`with_scene`](Self::with_scene)'s is:
    /// [`crate::texture`] records its own barriers and waits for the device to
    /// go idle, which is legal here because it runs between frames, outside
    /// any graph, and blocks until the copy has landed — so no frame can bind
    /// an image whose upload has not finished. See this crate's docs on the
    /// one rule.
    ///
    /// # Errors
    ///
    /// [`HalError::InvalidDescriptor`] for a layer of the wrong length (the
    /// check [`with_scene`](Self::with_scene) makes) or a layer count that is
    /// not the one in force, and [`HalError`] from the upload. Either way the
    /// renderer is unchanged: the page in force stays in force.
    pub fn replace_page(
        &mut self,
        device: &dyn Device,
        queue: QueueHandle,
        kind: PageKind,
        page: &crate::scene::PageDesc<'_>,
    ) -> Result<(), HalError> {
        page.check()?;
        let layers = page.layers(kind);
        let have = self.page_layers[kind.index()];
        if layers.len() != have {
            return Err(HalError::InvalidDescriptor(format!(
                "the {} page in force has {have} layer(s) and the replacement carries {}; a \
                 replacement keeps the count, so every material row's layer still names the \
                 same texture",
                kind.label(),
                layers.len()
            )));
        }
        if layers.is_empty() {
            return Ok(());
        }
        let (fresh, extent) =
            upload_page(device, queue, kind, page.extent(kind), layers, &mut || {})?;
        let stale = std::mem::replace(self.page_mut(kind), fresh);
        self.page_extents[kind.index()] = (extent, extent);
        self.retired_pages.push(RetiredPage::Page(stale));
        Ok(())
    }

    /// The page of `kind` in force.
    fn page_mut(&mut self, kind: PageKind) -> &mut UploadedTexture {
        match kind {
            PageKind::BaseColor => &mut self.base_color_page,
            PageKind::Normal => &mut self.normal_page,
            PageKind::MetallicRoughnessOcclusion => &mut self.mro_page,
            PageKind::Emissive => &mut self.emissive_page,
        }
    }

    /// What a slot's groups name once it has moved onto everything in force:
    /// [`ForwardRenderer::base_color_sampler`] and the four pages.
    pub(super) const fn page_bindings(&self) -> PageBindings {
        PageBindings {
            sampler: self.base_color_sampler,
            views: [
                self.base_color_page.view,
                self.normal_page.view,
                self.mro_page.view,
                self.emissive_page.view,
            ],
        }
    }

    /// Moves this frame's slot onto [`page_bindings`](Self::page_bindings),
    /// where [`set_anisotropy`](Self::set_anisotropy) or
    /// [`replace_page`](Self::replace_page) changed it since the slot last drew.
    ///
    /// Every group of the mesh layout the slot holds — the camera's, the depth
    /// prepass's and each shadow view's — is rebuilt from the entries it was
    /// built from with the page sampler and the four page views rewritten, and
    /// the occlusion cache is dropped, since it was built from the camera's
    /// entries and names the old ones too. **Every replacement is created
    /// before any group is destroyed**, so a refusal leaves the slot whole on
    /// what it had and the next `begin_frame` tries again. A replaced sampler
    /// or page is destroyed here the moment no slot names it.
    ///
    /// Called from `begin_frame_body` once the ring has rotated: that is the
    /// point at which this slot's previous submission has retired, which is
    /// what makes destroying its groups sound.
    pub(super) fn adopt_pages(&mut self, device: &dyn Device) -> Result<(), HalError> {
        let frame = self.frame;
        let bindings = self.page_bindings();
        if self.slot_pages[frame] == bindings {
            return Ok(());
        }
        let layout = self.mesh_layout;
        let mut fresh =
            Vec::with_capacity(2 * (1 + self.views.len()) + self.shadow_groups[frame].len());
        // Every view's pair — the primary camera's and each secondary view's —
        // then the shadow views, in the order the swap below walks them.
        let lists = std::iter::once(&mut self.primary)
            .chain(self.views.iter_mut().flatten())
            .flat_map(|view| {
                [
                    (&mut view.mesh_group_entries[frame], "mesh frame"),
                    (&mut view.prepass_group_entries[frame], "depth prepass"),
                ]
            })
            .chain(
                self.shadow_group_entries[frame]
                    .iter_mut()
                    .map(|entries| (entries, "shadow view")),
            );
        for (entries, label) in lists {
            match rebuilt_with_pages(device, layout, &bindings, entries, label) {
                Ok(group) => fresh.push(group),
                Err(error) => {
                    for group in fresh {
                        device.destroy_bind_group(group);
                    }
                    return Err(error);
                }
            }
        }
        let mut fresh = fresh.into_iter();
        let mut swap = |slot: &mut BindGroupHandle| {
            let stale = std::mem::replace(
                slot,
                fresh
                    .next()
                    .unwrap_or_else(|| unreachable!("one fresh group per entry list")),
            );
            device.destroy_bind_group(stale);
        };
        for view in std::iter::once(&mut self.primary).chain(self.views.iter_mut().flatten()) {
            swap(&mut view.mesh_groups[frame]);
            swap(&mut view.prepass_groups[frame]);
            if let Some((_, stale)) = view.screen_channel_groups[frame].take() {
                device.destroy_bind_group(stale);
            }
        }
        for group in &mut self.shadow_groups[frame] {
            swap(group);
        }
        self.slot_pages[frame] = bindings;
        let named = &self.slot_pages;
        self.retired_pages.retain(|retired| {
            if named.iter().any(|slot| retired.named_by(slot)) {
                true
            } else {
                retired.destroy(device);
                false
            }
        });
        Ok(())
    }
}

/// `entries` with the page sampler and every page view rewritten to
/// `bindings`, as a new group of `layout` — [`ForwardRenderer::adopt_pages`]'s
/// one step.
///
/// The entries are rewritten in place as well as built from, so the list stays
/// the one the slot's group was built from for the next rebuild.
fn rebuilt_with_pages(
    device: &dyn Device,
    layout: BindGroupLayoutHandle,
    bindings: &PageBindings,
    entries: &mut [BindGroupEntry],
    label: &str,
) -> Result<BindGroupHandle, HalError> {
    bindings.write_into(entries, label);
    device.create_bind_group(&BindGroupDesc {
        label: Some(label),
        layout,
        entries,
        variable_count: None,
    })
}
