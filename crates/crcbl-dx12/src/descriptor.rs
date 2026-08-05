//! CPU-visible descriptor heaps, and the allocator that hands out slots in
//! them.
//!
//! # Why this module exists at all
//!
//! `crcbl-mtl` creates an image view with one call — `MTLTexture` has a
//! `newTextureView…` method that returns another `MTLTexture` — and a sampler
//! the same way. **D3D12 has no view objects.** A view is a fixed-size record
//! the driver writes into a descriptor heap, named afterwards by the CPU
//! address of the slot it landed in, so `create_image_view` and
//! `create_sampler` need somewhere to put one before they can create anything
//! at all. That somewhere is this module.
//!
//! # Four heaps, because D3D12 has four kinds and will not mix them
//!
//! A descriptor heap is typed: `CreateRenderTargetView` writes only into an
//! `RTV` heap, `CreateSampler` only into a `SAMPLER` heap, and a shader
//! resource, unordered access or constant buffer view only into a
//! `CBV_SRV_UAV` one. There is no "any descriptor" heap, so there is one
//! allocator per kind and a [`Slot`] carries the kind it came from — which is
//! what makes freeing a slot into the wrong heap a thing that cannot be
//! written rather than a rule someone has to remember.
//!
//! # Every heap here is **CPU-visible only**
//!
//! `D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE` is deliberately never set. A
//! shader-visible heap is the one a root signature binds and the one SM6.6's
//! `ResourceDescriptorHeap` indexes, and there may be only one of each type
//! bound at a time — so it is a *frame* resource with a residency and
//! versioning scheme attached, and it belongs to the bind-group slice that has
//! a root signature to bind it against. What this slice needs is a staging
//! home for a descriptor that outlives the call that made it, and that is
//! exactly a non-shader-visible heap. Copying a range of them into a
//! shader-visible heap is `CopyDescriptorsSimple`, which is the call that slice
//! will make.
//!
//! # Chunks, not one big heap
//!
//! Heaps are allocated a chunk at a time and never freed until the device is,
//! with a free list of returned slots in front. The alternative — one heap
//! sized at device creation — has to pick a number, and every such number is
//! either an arbitrary ceiling a real scene walks into or a large allocation
//! made on every machine for the one that does not. Growing costs a
//! `CreateDescriptorHeap` per chunk and nothing per allocation.

use crcbl_hal::HalError;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
    D3D12_DESCRIPTOR_HEAP_TYPE, D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
    D3D12_DESCRIPTOR_HEAP_TYPE_DSV, D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
    D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER, ID3D12DescriptorHeap, ID3D12Device,
};

/// Descriptors in one heap.
///
/// Sized to be worth a `CreateDescriptorHeap` call without being a meaningful
/// allocation on its own: the widest descriptor D3D12 defines is a sampler, and
/// a chunk of them is still measured in kilobytes.
const DESCRIPTORS_PER_CHUNK: u32 = 256;

/// Which of D3D12's four descriptor heap types a slot lives in.
///
/// A seam-side name rather than the D3D12 constant, so a caller asks for what
/// it is making — a shader resource, a sampler, a render target, a depth
/// stencil — and [`Descriptors`] does the routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Shader resource, unordered access and constant buffer views. One heap
    /// type serves all three in D3D12.
    ShaderResource,
    /// Samplers.
    Sampler,
    /// Render target views.
    RenderTarget,
    /// Depth stencil views.
    DepthStencil,
}

impl Kind {
    /// The D3D12 heap type this kind names.
    const fn heap_type(self) -> D3D12_DESCRIPTOR_HEAP_TYPE {
        match self {
            Self::ShaderResource => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            Self::Sampler => D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
            Self::RenderTarget => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            Self::DepthStencil => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
        }
    }
}

/// One allocated descriptor: which heap kind, and where in it.
///
/// The kind rides along rather than being passed back in beside the index,
/// because the two are only ever correct together — a slot freed into another
/// kind's allocator would put a live descriptor's index on the wrong free list
/// and hand it out twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    kind: Kind,
    index: u32,
}

/// One descriptor heap, and the CPU address its first descriptor sits at.
#[derive(Debug)]
struct Chunk {
    /// Held because the descriptors written into it stay valid only while it
    /// is alive; nothing reads the interface back.
    _heap: ID3D12DescriptorHeap,
    base: usize,
}

/// The heaps of one kind, and the slots handed out from them.
#[derive(Debug)]
struct HeapAllocator {
    kind: Kind,
    /// `GetDescriptorHandleIncrementSize` for this heap type. A driver-specific
    /// number — D3D12 documents it as the one thing about a descriptor that
    /// must be asked for rather than assumed.
    stride: usize,
    chunks: Vec<Chunk>,
    /// Slots ever handed out. Never decreases; a returned slot joins
    /// `recycled` instead, so an index always names the same address.
    issued: u32,
    recycled: Vec<u32>,
}

impl HeapAllocator {
    fn new(device: &ID3D12Device, kind: Kind) -> Self {
        // SAFETY: `device` is a live `ID3D12Device` this crate owns a reference
        // to. `GetDescriptorHandleIncrementSize` reads no pointer of ours and
        // returns a `u32` by value; it cannot fail, which is why the binding
        // returns the number rather than a `Result`.
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(kind.heap_type()) };
        Self {
            kind,
            stride: stride as usize,
            chunks: Vec::new(),
            issued: 0,
            recycled: Vec::new(),
        }
    }

    /// Takes a slot, growing by one chunk if every issued one is in use.
    fn allocate(&mut self, device: &ID3D12Device) -> Result<Slot, HalError> {
        if let Some(index) = self.recycled.pop() {
            return Ok(Slot {
                kind: self.kind,
                index,
            });
        }
        let index = self.issued;
        if (index / DESCRIPTORS_PER_CHUNK) as usize >= self.chunks.len() {
            self.push_chunk(device)?;
        }
        self.issued = index.checked_add(1).ok_or(HalError::OutOfDeviceMemory)?;
        Ok(Slot {
            kind: self.kind,
            index,
        })
    }

    /// Returns a slot for reuse.
    fn free(&mut self, slot: Slot) {
        debug_assert_eq!(slot.kind, self.kind, "a slot returned to the wrong heap");
        self.recycled.push(slot.index);
    }

    fn push_chunk(&mut self, device: &ID3D12Device) -> Result<(), HalError> {
        let desc = D3D12_DESCRIPTOR_HEAP_DESC {
            Type: self.kind.heap_type(),
            NumDescriptors: DESCRIPTORS_PER_CHUNK,
            // Never shader-visible; see the module docs.
            Flags: D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
            // Node zero is the only one the seam models — `crcbl-hal` has no
            // multi-adapter vocabulary, so a linked-node rig is described as its
            // first node rather than wrongly.
            NodeMask: 0,
        };
        // SAFETY: `desc` is a live, fully initialised `D3D12_DESCRIPTOR_HEAP_DESC`
        // and the pointer is taken from it for the duration of the call only.
        // `ID3D12DescriptorHeap` is the IID asked for, so the call either writes
        // that interface or fails.
        let heap: ID3D12DescriptorHeap =
            unsafe { device.CreateDescriptorHeap(&desc) }.map_err(|error| {
                HalError::Backend(format!(
                    "CreateDescriptorHeap({:?}) failed: {error}",
                    self.kind
                ))
            })?;
        // SAFETY: `heap` is the interface the call just returned. The handle is
        // a plain address the driver reports by value and is stable for the
        // heap's lifetime, which is what makes caching it here sound.
        let base = unsafe { heap.GetCPUDescriptorHandleForHeapStart() }.ptr;
        self.chunks.push(Chunk { _heap: heap, base });
        Ok(())
    }

    /// The CPU address of a slot's descriptor.
    fn cpu_handle(&self, slot: Slot) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        debug_assert_eq!(
            slot.kind, self.kind,
            "a slot resolved against the wrong heap"
        );
        let chunk = (slot.index / DESCRIPTORS_PER_CHUNK) as usize;
        let offset = (slot.index % DESCRIPTORS_PER_CHUNK) as usize;
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: self.chunks[chunk].base + offset * self.stride,
        }
    }
}

/// Every descriptor heap the device owns, one allocator per D3D12 heap type.
#[derive(Debug)]
pub(crate) struct Descriptors {
    shader_resource: HeapAllocator,
    sampler: HeapAllocator,
    render_target: HeapAllocator,
    depth_stencil: HeapAllocator,
}

impl Descriptors {
    /// Reads each heap type's descriptor stride off the device. No heap is
    /// created until something asks for a slot.
    pub(crate) fn new(device: &ID3D12Device) -> Self {
        Self {
            shader_resource: HeapAllocator::new(device, Kind::ShaderResource),
            sampler: HeapAllocator::new(device, Kind::Sampler),
            render_target: HeapAllocator::new(device, Kind::RenderTarget),
            depth_stencil: HeapAllocator::new(device, Kind::DepthStencil),
        }
    }

    fn heap(&mut self, kind: Kind) -> &mut HeapAllocator {
        match kind {
            Kind::ShaderResource => &mut self.shader_resource,
            Kind::Sampler => &mut self.sampler,
            Kind::RenderTarget => &mut self.render_target,
            Kind::DepthStencil => &mut self.depth_stencil,
        }
    }

    /// Takes a descriptor of `kind`.
    ///
    /// # Errors
    ///
    /// [`HalError::Backend`] if D3D12 refused a new heap, or
    /// [`HalError::OutOfDeviceMemory`] if this kind has issued every index a
    /// `u32` can name.
    pub(crate) fn allocate(&mut self, device: &ID3D12Device, kind: Kind) -> Result<Slot, HalError> {
        self.heap(kind).allocate(device)
    }

    /// Returns a descriptor for reuse. The bytes it names are left alone: a
    /// stale descriptor is only reachable through a handle that no longer
    /// resolves.
    pub(crate) fn free(&mut self, slot: Slot) {
        self.heap(slot.kind).free(slot);
    }

    /// The CPU address to write a descriptor at.
    pub(crate) fn cpu_handle(&mut self, slot: Slot) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        self.heap(slot.kind).cpu_handle(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::tests::open_device;

    /// Every kind, so the properties below cover all four heaps rather than the
    /// one that was convenient.
    const KINDS: &[Kind] = &[
        Kind::ShaderResource,
        Kind::Sampler,
        Kind::RenderTarget,
        Kind::DepthStencil,
    ];

    /// **Distinct live slots have distinct addresses, across a chunk
    /// boundary.**
    ///
    /// The address is the thing under test and the reason the loop runs past
    /// [`DESCRIPTORS_PER_CHUNK`]: within one chunk almost any indexing bug
    /// still produces distinct pointers, and it is the second chunk — a
    /// separate heap at an unrelated base address — where an allocator that
    /// forgot which chunk a slot belongs to starts handing out the *first*
    /// chunk's addresses again. Two views then share one descriptor, which no
    /// call reports, because `CreateShaderResourceView` returns nothing.
    #[test]
    fn slots_have_distinct_addresses_across_a_chunk_boundary() {
        let (_instance, device) = open_device();
        let raw = device.raw().clone();
        let mut descriptors = Descriptors::new(&raw);
        let count = DESCRIPTORS_PER_CHUNK + DESCRIPTORS_PER_CHUNK / 2;

        for &kind in KINDS {
            let mut slots = Vec::new();
            let mut addresses = Vec::new();
            for index in 0..count {
                let slot = descriptors
                    .allocate(&raw, kind)
                    .unwrap_or_else(|error| panic!("{kind:?} slot {index}: {error:?}"));
                let ptr = descriptors.cpu_handle(slot).ptr;
                assert_ne!(
                    ptr, 0,
                    "{kind:?} slot {index} has a null descriptor address"
                );
                assert!(
                    !addresses.contains(&ptr),
                    "{kind:?} slot {index} reuses address {ptr:#x} of a live descriptor"
                );
                addresses.push(ptr);
                slots.push(slot);
            }
            assert_eq!(
                addresses.len(),
                count as usize,
                "{kind:?}: a slot was skipped"
            );
            for slot in slots {
                descriptors.free(slot);
            }
        }
    }

    /// A freed slot comes back, and comes back at the same address — which is
    /// what makes the free list a recycler rather than a leak with extra steps.
    #[test]
    fn a_freed_slot_is_reissued_at_the_address_it_had() {
        let (_instance, device) = open_device();
        let raw = device.raw().clone();
        let mut descriptors = Descriptors::new(&raw);

        let first = descriptors
            .allocate(&raw, Kind::ShaderResource)
            .expect("the first descriptor");
        let second = descriptors
            .allocate(&raw, Kind::ShaderResource)
            .expect("the second descriptor");
        let first_ptr = descriptors.cpu_handle(first).ptr;
        assert_ne!(first_ptr, descriptors.cpu_handle(second).ptr);

        descriptors.free(first);
        let reissued = descriptors
            .allocate(&raw, Kind::ShaderResource)
            .expect("the recycled descriptor");
        assert_eq!(reissued, first, "the free list did not hand the slot back");
        assert_eq!(
            descriptors.cpu_handle(reissued).ptr,
            first_ptr,
            "a recycled slot moved, so an index does not name a fixed address"
        );

        // And the still-live slot is untouched by the round trip.
        let third = descriptors
            .allocate(&raw, Kind::ShaderResource)
            .expect("a fresh descriptor");
        assert_ne!(third, second);
        assert_ne!(descriptors.cpu_handle(third).ptr, first_ptr);
    }

    /// The four kinds are four heaps, so a slot in one says nothing about a
    /// slot in another.
    ///
    /// Written on addresses rather than on indices: every allocator starts at
    /// index zero, so comparing indices would pass however the heaps were
    /// wired. Two kinds sharing a heap would put a sampler descriptor where a
    /// render target view belongs, which D3D12 refuses at *bind* time and never
    /// at write time.
    #[test]
    fn each_kind_allocates_out_of_its_own_heap() {
        let (_instance, device) = open_device();
        let raw = device.raw().clone();
        let mut descriptors = Descriptors::new(&raw);

        let mut addresses = Vec::new();
        assert!(!KINDS.is_empty(), "nothing to check");
        for &kind in KINDS {
            let slot = descriptors
                .allocate(&raw, kind)
                .unwrap_or_else(|error| panic!("{kind:?}: {error:?}"));
            assert_eq!(slot.index, 0, "{kind:?}: each heap counts from zero");
            let ptr = descriptors.cpu_handle(slot).ptr;
            assert!(
                !addresses.contains(&ptr),
                "{kind:?} allocated at {ptr:#x}, which another kind already owns"
            );
            addresses.push(ptr);
        }
        assert_eq!(addresses.len(), KINDS.len());
    }
}
