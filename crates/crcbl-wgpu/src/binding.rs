//! Turning a HAL bind group into wgpu's.
//!
//! The two descriptors count different things. The seam's
//! [`BindGroupEntry`](crcbl_hal::BindGroupEntry) is one entry per **array
//! element** — `array_index` is the bindless write path, one entry per texture
//! slot updated — while wgpu's is one entry per **binding**, whose resource is
//! either a single object or a slice of them. Handing wgpu one entry per HAL
//! entry gives two `wgpu::BindGroupEntry`s with the same `binding`, which it
//! rejects.
//!
//! # wgpu's arrays are dense, and the seam's are not
//!
//! Element *i* of a `TextureViewArray` **is** array element *i*: there is no
//! index alongside it and no sparse spelling. The seam permits a partial fill
//! (see [`BindGroupDesc::entries`]), so a group naming elements 0 and 2 is a
//! legal descriptor with no wgpu counterpart — collapsing it to a two-element
//! slice would bind element 2's resource into element 1 and produce a wrong
//! picture with no error anywhere. Every hole is refused here instead.
//!
//! A *trailing* shortfall is the one partial fill wgpu does accept: elements
//! `0..n` with `n` below the layout's count, under
//! `wgpu::Features::PARTIALLY_BOUND_BINDING_ARRAY`. That one is passed through.
//!
//! # `variable_count` is checked against the entries, not obeyed
//!
//! [`BindGroupDesc::variable_count`] says how many elements of a
//! [`VARIABLE_COUNT`](crcbl_hal::BindingFlags::VARIABLE_COUNT) array the group
//! uses. On Vulkan that number *sizes an allocation*: the set is allocated with
//! `n` slots and the ones this descriptor leaves empty are written later
//! through [`update_bind_group`](crcbl_hal::Device::update_bind_group).
//!
//! wgpu has neither half. A binding array's length **is** the length of the
//! slice handed to `create_bind_group` — that is what the trailing shortfall
//! above is — and this backend's `update_bind_group` is
//! [`HalError::Unsupported`] because WebGPU bind groups are immutable once
//! created. So an explicit `n` chooses nothing the entry list has not already
//! said, and the two ways to "honour" it both fail: dropping it is the silent
//! downgrade this module exists to refuse, and padding the slice out to `n`
//! would bind resources the caller never named. It is checked instead, by
//! [`check_variable_count`] — an `n` the entries contradict is a caller bug
//! this backend can name today, whichever way the seam's field grows later.

use crcbl_hal::{self as hal, BindGroupDesc, HalError};

use crate::handle::{self, Owner};
use crate::resources::{BindGroupLayoutSlot, Pools};

/// A resolved buffer range: `wgpu::BufferBinding` with the buffer owned rather
/// than borrowed, so the pool guard is dropped long before the descriptor that
/// borrows it is built.
struct BufferRange {
    buffer: wgpu::Buffer,
    offset: u64,
    size: Option<std::num::NonZero<u64>>,
}

/// One binding's resolved resources, all of one kind.
///
/// Kind-homogeneous by construction because wgpu's array spellings are typed:
/// `TextureViewArray` takes views and nothing else. A binding filled with a
/// mixture has no wgpu spelling at all, so this type has nowhere to put one and
/// [`resolve`] says so rather than picking a variant from the first entry and
/// dropping the rest.
enum Resources {
    Buffers(Vec<BufferRange>),
    Views(Vec<wgpu::TextureView>),
    Samplers(Vec<wgpu::Sampler>),
}

/// Everything one binding number contributes to a bind group.
pub(crate) struct Bound {
    binding: u32,
    /// Whether wgpu wants the array spelling. Read off the layout's declared
    /// count, never off `resources.len()`.
    array: bool,
    resources: Resources,
}

impl Bound {
    /// How many array elements this binding was given.
    fn elements(&self) -> u32 {
        let count = match &self.resources {
            Resources::Buffers(buffers) => buffers.len(),
            Resources::Views(views) => views.len(),
            Resources::Samplers(samplers) => samplers.len(),
        };
        count as u32
    }
}

/// The same resources, borrowed in the shapes `wgpu::BindingResource` takes.
enum Borrowed<'a> {
    Buffers(Vec<wgpu::BufferBinding<'a>>),
    Views(Vec<&'a wgpu::TextureView>),
    Samplers(Vec<&'a wgpu::Sampler>),
}

/// Buckets a descriptor's entries by binding number and resolves each bucket's
/// resources, or says why the group cannot be built.
///
/// Buckets come back sorted by binding and each bucket's elements sorted by
/// `array_index`, so one descriptor always produces one wgpu descriptor whatever
/// order the caller listed its entries in.
pub(crate) fn resolve(
    desc: &BindGroupDesc<'_>,
    layout: &BindGroupLayoutSlot,
    pools: &Pools,
    owner: Owner,
) -> Result<Vec<Bound>, HalError> {
    let mut buckets: Vec<(u32, Vec<&hal::BindGroupEntry>)> = Vec::new();
    for entry in desc.entries {
        match buckets
            .iter_mut()
            .find(|(binding, _)| *binding == entry.binding)
        {
            Some((_, bucket)) => bucket.push(entry),
            None => buckets.push((entry.binding, vec![entry])),
        }
    }
    buckets.sort_by_key(|(binding, _)| *binding);

    let mut bound = Vec::with_capacity(buckets.len());
    for (binding, mut entries) in buckets {
        let Some(count) = layout.count(binding) else {
            return Err(HalError::InvalidDescriptor(format!(
                "bind group entry names binding {binding}, which the layout does not declare"
            )));
        };
        // `create_bind_group_layout` gives wgpu a count only when the seam
        // declared more than one, so the same comparison decides the spelling
        // here. The count is taken as it stands: a zero is refused when the
        // layout is created, and clamping it up here would have silently
        // admitted one write to a binding holding no descriptors at all.
        let array = count > 1;

        entries.sort_by_key(|entry| entry.array_index);
        for (position, entry) in entries.iter().enumerate() {
            let position = position as u32;
            if entry.array_index >= count {
                return Err(HalError::InvalidDescriptor(format!(
                    "bind group entry writes index {} of binding {binding}, which holds {count}",
                    entry.array_index
                )));
            }
            // Sorted, so the run of indices must read 0, 1, 2, … — anything
            // below its position is a repeat and anything above it leaves a
            // hole. `InvalidDescriptor` rather than `Unsupported` for both:
            // `HalError::Unsupported` carries a `&'static str` and could not
            // name the binding or the index, which is the whole value of the
            // message.
            if entry.array_index < position {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {binding} index {} is written twice by one bind group",
                    entry.array_index
                )));
            }
            if entry.array_index > position {
                return Err(HalError::InvalidDescriptor(format!(
                    "binding {binding} skips index {position} and writes index {} instead; wgpu \
                     has no sparse array binding — element i of the slice *is* array element i, \
                     so a hole would silently shift every later element down one",
                    entry.array_index
                )));
            }
        }

        let mut resources: Option<Resources> = None;
        for entry in entries {
            let slot = resources.get_or_insert_with(|| match entry.resource {
                hal::BindingResource::Buffer { .. } => Resources::Buffers(Vec::new()),
                hal::BindingResource::ImageView(_) => Resources::Views(Vec::new()),
                hal::BindingResource::Sampler(_) => Resources::Samplers(Vec::new()),
            });
            match (slot, entry.resource) {
                (
                    Resources::Buffers(buffers),
                    hal::BindingResource::Buffer {
                        buffer,
                        offset,
                        size,
                    },
                ) => {
                    let bufs = pools.buffers.lock().unwrap();
                    let buffer = handle::lookup(&bufs, "buffer", buffer, owner)?
                        .buffer
                        .clone();
                    let size = if size == hal::BindingResource::WHOLE_BUFFER {
                        None
                    } else {
                        Some(std::num::NonZero::new(size).ok_or_else(|| {
                            HalError::InvalidDescriptor(format!(
                                "binding {binding} names a zero-sized range of a buffer; use \
                                 BindingResource::whole_buffer for the whole thing"
                            ))
                        })?)
                    };
                    buffers.push(BufferRange {
                        buffer,
                        offset,
                        size,
                    });
                }
                (Resources::Views(views), hal::BindingResource::ImageView(view)) => {
                    let pool = pools.image_views.lock().unwrap();
                    views.push(handle::lookup(&pool, "image view", view, owner)?.clone());
                }
                (Resources::Samplers(samplers), hal::BindingResource::Sampler(sampler)) => {
                    let pool = pools.samplers.lock().unwrap();
                    samplers.push(handle::lookup(&pool, "sampler", sampler, owner)?.clone());
                }
                _ => {
                    return Err(HalError::InvalidDescriptor(format!(
                        "binding {binding} is filled with more than one kind of resource; wgpu's \
                         array bindings are typed, so one binding number holds buffers, image \
                         views or samplers and never a mixture"
                    )));
                }
            }
        }
        let Some(resources) = resources else {
            unreachable!("a bucket exists because an entry created it, so it is never empty")
        };
        bound.push(Bound {
            binding,
            array,
            resources,
        });
    }
    check_variable_count(desc, layout, &bound)?;
    Ok(bound)
}

/// Checks [`BindGroupDesc::variable_count`] against what the group supplies, or
/// says why the two cannot both be true.
///
/// See the module docs on why agreeing with the entries is the whole of what
/// the field can mean here.
///
/// The binding it describes is the one the layout declared `VARIABLE_COUNT` on,
/// which [`BindGroupLayoutSlot`] recorded at creation because wgpu's layout
/// cannot be asked for it. A `variable_count` on a layout that declares no such
/// binding is refused in `crcbl-vk`'s words, because it is the same caller bug.
fn check_variable_count(
    desc: &BindGroupDesc<'_>,
    layout: &BindGroupLayoutSlot,
    bound: &[Bound],
) -> Result<(), HalError> {
    let Some(requested) = desc.variable_count else {
        return Ok(());
    };
    let Some(binding) = layout.variable_binding() else {
        return Err(HalError::InvalidDescriptor(
            "BindGroupDesc::variable_count on a layout that declares no VARIABLE_COUNT binding"
                .to_string(),
        ));
    };
    let ceiling = layout
        .count(binding)
        .unwrap_or_else(|| unreachable!("the variable binding came from the layout's own entries"));
    // Zero when the group names that binding nowhere, which is a disagreement
    // like any other rather than a case of its own.
    let elements = bound
        .iter()
        .find(|slot| slot.binding == binding)
        .map_or(0, Bound::elements);
    if requested != elements {
        return Err(HalError::InvalidDescriptor(format!(
            "BindGroupDesc::variable_count is {requested}, but this group fills {elements} of \
             binding {binding}'s {ceiling} elements; wgpu takes a binding array's length from the \
             entry list itself and this backend has no update_bind_group, so a slot the \
             descriptor leaves empty can never be filled later"
        )));
    }
    Ok(())
}

/// Runs `build` on the `wgpu::BindGroupEntry`s `bound` describes.
///
/// A closure rather than a returned `Vec`: every array spelling borrows a slice
/// of references that has to outlive the entries built from it, and that slice
/// can only live on this function's own stack.
pub(crate) fn with_entries<T>(
    bound: &[Bound],
    build: impl FnOnce(&[wgpu::BindGroupEntry<'_>]) -> T,
) -> T {
    let borrowed: Vec<Borrowed<'_>> = bound
        .iter()
        .map(|slot| match &slot.resources {
            Resources::Buffers(buffers) => Borrowed::Buffers(
                buffers
                    .iter()
                    .map(|range| wgpu::BufferBinding {
                        buffer: &range.buffer,
                        offset: range.offset,
                        size: range.size,
                    })
                    .collect(),
            ),
            Resources::Views(views) => Borrowed::Views(views.iter().collect()),
            Resources::Samplers(samplers) => Borrowed::Samplers(samplers.iter().collect()),
        })
        .collect();

    let entries: Vec<wgpu::BindGroupEntry<'_>> = bound
        .iter()
        .zip(&borrowed)
        .map(|(slot, borrowed)| wgpu::BindGroupEntry {
            binding: slot.binding,
            resource: if slot.array {
                match borrowed {
                    Borrowed::Buffers(buffers) => wgpu::BindingResource::BufferArray(buffers),
                    Borrowed::Views(views) => wgpu::BindingResource::TextureViewArray(views),
                    Borrowed::Samplers(samplers) => wgpu::BindingResource::SamplerArray(samplers),
                }
            } else {
                match borrowed {
                    Borrowed::Buffers(buffers) => {
                        buffers.first().cloned().map(wgpu::BindingResource::Buffer)
                    }
                    Borrowed::Views(views) => views
                        .first()
                        .copied()
                        .map(wgpu::BindingResource::TextureView),
                    Borrowed::Samplers(samplers) => samplers
                        .first()
                        .copied()
                        .map(wgpu::BindingResource::Sampler),
                }
                .unwrap_or_else(|| {
                    unreachable!("a scalar binding holds the one entry that created its bucket")
                })
            },
        })
        .collect();

    build(&entries)
}
