//! One D3D12 adapter, described in the seam's vocabulary.
//!
//! Everything here is a **read**: each function asks DXGI or D3D12 a question
//! and turns the answer into [`AdapterInfo`] / [`DeviceCaps`]. Nothing is kept —
//! the `ID3D12Device` opened to ask the questions is dropped before this module
//! returns.
//!
//! # D3D12 has no adapter-level capability query, and that shapes this module
//!
//! Metal reads its answers straight off an `MTLDevice` that enumeration already
//! handed back, and Vulkan answers everything from
//! `vkGetPhysicalDeviceFeatures2`. D3D12 has neither:
//! `ID3D12Device::CheckFeatureSupport` is the *only* capability query, it lives
//! on the device, and there is no physical-device object in front of it. So
//! filling in [`AdapterInfo::caps`] means calling `D3D12CreateDevice` per
//! adapter, asking, and dropping the device again.
//!
//! That is the documented way to do it and it is what the seam's promise costs
//! here — [`Instance::adapters`](crcbl_hal::Instance::adapters) guarantees caps
//! are known *before* a device exists, so the alternative is a lie. It is paid
//! once per process at start-up, and it is also the reason
//! [`describe`] can answer `None`: an adapter DXGI lists but D3D12 will not open
//! (a display-only adapter, or a driver too old for the feature level) is one
//! this backend must not enumerate at all.
//!
//! # The rule this module follows
//!
//! **Report what the adapter answers; leave off what would be a promise about
//! code that is not written.** D3D12 is generous with universal guarantees —
//! every device has compute, `ID3D12Fence`, PIX markers — and it is tempting to
//! advertise them because they are true of the *API*. They are not yet true of
//! this *backend*: a caller that keyed off one of those flags would find the
//! entry point behind it refusing. `crcbl-mtl` reached the same conclusion and
//! states it the same way.
//!
//! A flag moves when the call behind it lands, and the resource slice moved two:
//! `TEXTURE_COMPRESSION_BC`, now that `create_image` creates BC images, and
//! `SAMPLER_ANISOTROPY`, now that `create_sampler` fills in a `MaxAnisotropy`.
//! Every reported flag, and every withheld one, is argued on [`features_of`].

use crcbl_hal::{
    AdapterId, AdapterInfo, BackendKind, DeviceCaps, DeviceType, Features, Format, Limits,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D_SHADER_MODEL, D3D_SHADER_MODEL_5_1, D3D_SHADER_MODEL_6_0, D3D_SHADER_MODEL_6_1,
    D3D_SHADER_MODEL_6_2, D3D_SHADER_MODEL_6_3, D3D_SHADER_MODEL_6_4, D3D_SHADER_MODEL_6_5,
    D3D_SHADER_MODEL_6_6, D3D_SHADER_MODEL_6_7, D3D_SHADER_MODEL_6_8, D3D_SHADER_MODEL_6_9,
    D3D_SHADER_MODEL_NONE, D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT,
    D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION, D3D12_CS_THREAD_GROUP_MAX_THREADS_PER_GROUP,
    D3D12_CS_THREAD_GROUP_MAX_X, D3D12_CS_THREAD_GROUP_MAX_Y, D3D12_CS_THREAD_GROUP_MAX_Z,
    D3D12_FEATURE, D3D12_FEATURE_ARCHITECTURE1, D3D12_FEATURE_D3D12_OPTIONS,
    D3D12_FEATURE_DATA_ARCHITECTURE1, D3D12_FEATURE_DATA_D3D12_OPTIONS,
    D3D12_FEATURE_DATA_FORMAT_SUPPORT, D3D12_FEATURE_DATA_SHADER_MODEL,
    D3D12_FEATURE_FORMAT_SUPPORT, D3D12_FEATURE_SHADER_MODEL, D3D12_FORMAT_SUPPORT1_SHADER_SAMPLE,
    D3D12_FORMAT_SUPPORT1_TEXTURE2D, D3D12_RAW_UAV_SRV_BYTE_ALIGNMENT,
    D3D12_REQ_CONSTANT_BUFFER_ELEMENT_COUNT, D3D12_REQ_MAXANISOTROPY,
    D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION, D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION,
    D3D12_REQ_TEXTURE3D_U_V_OR_W_DIMENSION, D3D12_RESOURCE_BINDING_TIER,
    D3D12_RESOURCE_BINDING_TIER_3, D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT,
    D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT, D3D12CreateDevice, ID3D12Device,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIDevice,
};
use windows::core::Interface;

use crate::conv;

/// Bytes in one constant-buffer element.
///
/// D3D12 states the constant-buffer ceiling as an element *count*
/// ([`D3D12_REQ_CONSTANT_BUFFER_ELEMENT_COUNT`]), where an element is one
/// four-component 32-bit vector — the unit HLSL's `cbuffer` packing rules are
/// written in. The headers give no byte figure, so the multiplication happens
/// here, and this is the factor it is named for rather than a literal in the
/// expression.
const CONSTANT_BUFFER_ELEMENT_BYTES: u64 = 4 * 4;

/// Microsoft's PCI vendor id, as DXGI reports it for the adapters Windows
/// itself provides.
///
/// Read off the `windows-latest` runner's own enumeration line rather than
/// recalled: both adapters it lists carry this vendor together with
/// [`BASIC_RENDER_DRIVER_DEVICE_ID`]. Consulted by [`is_software`] because
/// `DXGI_ADAPTER_FLAG_SOFTWARE` is clear on that machine and so cannot be
/// trusted on its own.
pub(crate) const MICROSOFT_VENDOR_ID: u32 = 0x1414;

/// The device id "Microsoft Basic Render Driver" — WARP — reports.
///
/// Same source as [`MICROSOFT_VENDOR_ID`], and only meaningful beside it: a
/// device id is unique within a vendor, never on its own.
pub(crate) const BASIC_RENDER_DRIVER_DEVICE_ID: u32 = 0x008c;

/// Shader models to ask about, newest first.
///
/// [`D3D12_FEATURE_SHADER_MODEL`] is an **in/out** query: the caller writes the
/// highest model it is prepared to hear about and the runtime lowers the value
/// to what the device supports. What it does *not* do is lower a model it has
/// never heard of — an older Windows answers `E_INVALIDARG` for that — so a
/// single call seeded with the newest constant in the bindings fails outright on
/// a machine that would have answered a real number. Descending until one call
/// succeeds is the pattern D3D12's own documentation gives, and every entry
/// below is a constant from the bindings rather than a number written here.
const PROBED_SHADER_MODELS: [D3D_SHADER_MODEL; 11] = [
    D3D_SHADER_MODEL_6_9,
    D3D_SHADER_MODEL_6_8,
    D3D_SHADER_MODEL_6_7,
    D3D_SHADER_MODEL_6_6,
    D3D_SHADER_MODEL_6_5,
    D3D_SHADER_MODEL_6_4,
    D3D_SHADER_MODEL_6_3,
    D3D_SHADER_MODEL_6_2,
    D3D_SHADER_MODEL_6_1,
    D3D_SHADER_MODEL_6_0,
    D3D_SHADER_MODEL_5_1,
];

/// The raw D3D12 answers every derived capability on this adapter comes from.
///
/// Kept beside the [`AdapterInfo`] rather than discarded, for one reason:
/// `docs/backlog.md`'s "Does WARP clear Tier A?" is answered by these numbers
/// and not by the flags derived from them, so the tests have to be able to
/// **print what the adapter said** and to check the derivation against its
/// inputs. A test that only compared derived flags with each other could not
/// tell a right rule from a wrong one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RawCaps {
    /// `D3D12_FEATURE_DATA_D3D12_OPTIONS::ResourceBindingTier`.
    pub(crate) binding_tier: D3D12_RESOURCE_BINDING_TIER,
    /// `D3D12_FEATURE_DATA_SHADER_MODEL::HighestShaderModel`, probed.
    pub(crate) shader_model: D3D_SHADER_MODEL,
    /// Whether this adapter is a software rasteriser — which, on a stock
    /// Windows, means it is WARP. [`is_software`]'s answer, so it is the flag
    /// **and** the known Microsoft ids rather than the flag alone.
    pub(crate) software: bool,
    /// `D3D12_FEATURE_DATA_ARCHITECTURE1::UMA`: the GPU shares the CPU's memory
    /// pool.
    pub(crate) unified_memory: bool,
    /// `DXGI_ADAPTER_DESC1::AdapterLuid`, packed — **the only identity DXGI
    /// actually guarantees**, locally unique for the lifetime of the system and
    /// equal for two interfaces onto one adapter.
    ///
    /// Kept because name, vendor and device id are *not* an identity: two
    /// genuinely distinct cards of one model share all three, and the runner
    /// lists two adapters that share all three and are still two adapters — they
    /// carry different LUIDs. Enumeration de-duplicates on this, and a test
    /// asserts no two adapters share one.
    pub(crate) luid: u64,
    /// Whether every BC format the seam declares can be sampled as a 2D texture
    /// on this adapter, from `D3D12_FEATURE_FORMAT_SUPPORT`.
    ///
    /// Asked per format because that is the only way D3D12 answers it, and
    /// answered as one flag because [`Features::TEXTURE_COMPRESSION_BC`] is one
    /// flag — a partial answer would be a feature that is true of some of the
    /// formats it names, which is worse than either.
    pub(crate) block_compression: bool,
}

impl RawCaps {
    /// Whether this adapter supports **SM6.6 dynamic resources** —
    /// `ResourceDescriptorHeap[i]` and `SamplerDescriptorHeap[i]` in HLSL.
    ///
    /// D3D12 requires *both* halves and neither implies the other: the shader
    /// model is what makes the syntax exist, and
    /// [`D3D12_RESOURCE_BINDING_TIER_3`] is what makes a descriptor heap large
    /// enough and unbounded enough to index into. A device can report SM6.6 on
    /// tier 2 hardware, which is exactly the combination that would compile and
    /// then fail to bind.
    ///
    /// This is the answer `docs/backlog.md` asked for about WARP, and the whole
    /// reason [`RawCaps`] is kept.
    pub(crate) fn dynamic_resources(self) -> bool {
        self.binding_tier.0 >= D3D12_RESOURCE_BINDING_TIER_3.0
            && self.shader_model.0 >= D3D_SHADER_MODEL_6_6.0
    }
}

/// A `CheckFeatureSupport` query, paired with the struct D3D12 fills in for it.
///
/// `CheckFeatureSupport` is a void-pointer-and-size ABI, so passing
/// [`D3D12_FEATURE_D3D12_OPTIONS`] with a `D3D12_FEATURE_DATA_SHADER_MODEL`
/// compiles. D3D12 checks the *size* against the feature and refuses a mismatch,
/// which is why this is a correctness hazard rather than a memory-safety one —
/// and it stops being even that between two payloads of the same width, where
/// the driver fills in fields that mean something else entirely. **The pairing is
/// enforced here rather than documented**, so a call site cannot name one without
/// the other, and [`check_feature`] never has to be trusted to have been called
/// correctly.
trait FeatureQuery: Copy {
    /// The `D3D12_FEATURE` this struct is the payload of.
    const FEATURE: D3D12_FEATURE;
}

impl FeatureQuery for D3D12_FEATURE_DATA_D3D12_OPTIONS {
    const FEATURE: D3D12_FEATURE = D3D12_FEATURE_D3D12_OPTIONS;
}

impl FeatureQuery for D3D12_FEATURE_DATA_SHADER_MODEL {
    const FEATURE: D3D12_FEATURE = D3D12_FEATURE_SHADER_MODEL;
}

impl FeatureQuery for D3D12_FEATURE_DATA_ARCHITECTURE1 {
    const FEATURE: D3D12_FEATURE = D3D12_FEATURE_ARCHITECTURE1;
}

impl FeatureQuery for D3D12_FEATURE_DATA_FORMAT_SUPPORT {
    const FEATURE: D3D12_FEATURE = D3D12_FEATURE_FORMAT_SUPPORT;
}

/// One `CheckFeatureSupport` call, with the answer returned rather than written
/// through a pointer.
///
/// `data` goes **in** as well as out: several D3D12 feature structs are in/out
/// (`SHADER_MODEL` carries the model being asked about, `ARCHITECTURE1` carries
/// the node index), so a signature that only returned a value could not express
/// them. Callers that have nothing to say pass `T::default()`.
///
/// `None` when the driver refused the query, which is a legitimate answer rather
/// than an error: a feature struct newer than the runtime reports `E_INVALIDARG`
/// and the caller falls back.
fn check_feature<T: FeatureQuery>(device: &ID3D12Device, mut data: T) -> Option<T> {
    let size = u32::try_from(size_of::<T>()).ok()?;
    // SAFETY: `data` is a live, initialised `T` and `size` is that same `T`'s
    // size, so every byte the driver writes lands inside the local. The other
    // half of the contract — that `T` is the payload D3D12 documents for the
    // feature id — is discharged by the `FeatureQuery` bound rather than by this
    // comment: the id comes from the type.
    let result = unsafe {
        device.CheckFeatureSupport(T::FEATURE, core::ptr::from_mut(&mut data).cast(), size)
    };
    match result {
        Ok(()) => Some(data),
        Err(error) => {
            crcbl_core::log::debug!(
                "crcbl-dx12: CheckFeatureSupport({:?}) refused: {error}",
                T::FEATURE
            );
            None
        }
    }
}

/// The highest shader model this device admits to, or [`D3D_SHADER_MODEL_NONE`]
/// if it admits to none of the probed ones.
///
/// See [`PROBED_SHADER_MODELS`] for why this is a descending probe and not one
/// call.
fn highest_shader_model(device: &ID3D12Device) -> D3D_SHADER_MODEL {
    for probe in PROBED_SHADER_MODELS {
        let asked = D3D12_FEATURE_DATA_SHADER_MODEL {
            HighestShaderModel: probe,
        };
        if let Some(answer) = check_feature(device, asked) {
            // The runtime lowers the value it was given, so the answer is the
            // device's own ceiling and not the probe that reached it.
            return answer.HighestShaderModel;
        }
    }
    D3D_SHADER_MODEL_NONE
}

/// Every BC format the seam declares.
///
/// Hand-written because [`Format`] has no iterator. Only the BC arms appear:
/// `supports_block_compression` asks about exactly the formats
/// [`Features::TEXTURE_COMPRESSION_BC`] promises, and a list that reached wider
/// would report the feature on the strength of formats it does not name.
const BC_FORMATS: [Format; 9] = [
    Format::Bc1RgbaUnorm,
    Format::Bc1RgbaUnormSrgb,
    Format::Bc3RgbaUnorm,
    Format::Bc3RgbaUnormSrgb,
    Format::Bc4RUnorm,
    Format::Bc5RgUnorm,
    Format::Bc6hRgbUfloat,
    Format::Bc7RgbaUnorm,
    Format::Bc7RgbaUnormSrgb,
];

/// Whether every BC format can be a sampled 2D texture on this device.
///
/// Both support bits are required and neither implies the other:
/// `D3D12_FORMAT_SUPPORT1_TEXTURE2D` says a texture of that format can exist,
/// and `_SHADER_SAMPLE` says a shader may sample it. A format that could be
/// created and not sampled would pass a one-bit check and then produce a
/// texture the renderer cannot read.
///
/// **A refused query degrades to "no".** `D3D12_FEATURE_DATA_FORMAT_SUPPORT`
/// answers per format and a driver that will not answer loses the feature rather
/// than gaining it, which is the direction a capability must fail in.
fn supports_block_compression(device: &ID3D12Device) -> bool {
    BC_FORMATS.iter().all(|&format| {
        let asked = D3D12_FEATURE_DATA_FORMAT_SUPPORT {
            Format: conv::dxgi_format(format),
            ..Default::default()
        };
        check_feature(device, asked).is_some_and(|answer| {
            answer.Support1.contains(D3D12_FORMAT_SUPPORT1_TEXTURE2D)
                && answer
                    .Support1
                    .contains(D3D12_FORMAT_SUPPORT1_SHADER_SAMPLE)
        })
    })
}

/// A shader model as `"major.minor"`.
///
/// `D3D_SHADER_MODEL`'s values encode the version one nibble each — the low
/// nibble is the minor version and the one above it the major — which is why
/// [`D3D_SHADER_MODEL_6_6`] and its neighbours are a contiguous run.
/// `the_shader_model_decoding_matches_the_constants_it_decodes` checks that
/// against the constants themselves rather than against this sentence.
pub(crate) fn shader_model_name(model: D3D_SHADER_MODEL) -> String {
    if model == D3D_SHADER_MODEL_NONE {
        return "none".to_string();
    }
    format!("{}.{}", model.0 >> 4, model.0 & 0xf)
}

/// What D3D12 reports as its driver version.
///
/// There is no driver version in `DXGI_ADAPTER_DESC1` and none in D3D12 at all.
/// The one thing DXGI will answer is
/// `IDXGIAdapter::CheckInterfaceSupport(IID_IDXGIDevice)`, which hands back the
/// **user-mode driver version** packed into an `i64`, four 16-bit fields most to
/// least significant — the number vendor bug reports are filed against. Adapters
/// with no Direct3D 10-class user-mode driver refuse it, WARP being the case to
/// expect, so the refusal is a string rather than a failure: this field is for
/// logs and bug reports, and losing the whole adapter over a missing version
/// would be absurd.
fn driver_string(adapter: &IDXGIAdapter1) -> String {
    // SAFETY: `adapter` is a live COM interface this crate owns a reference to,
    // and the IID is a `'static` constant from the bindings.
    // `CheckInterfaceSupport` reads nothing beyond its own arguments and writes
    // only the `i64` the binding returns by value.
    let packed = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) };
    match packed {
        Ok(version) => {
            let bits = version.cast_unsigned();
            format!(
                "D3D12 UMD {}.{}.{}.{}",
                (bits >> 48) & 0xffff,
                (bits >> 32) & 0xffff,
                (bits >> 16) & 0xffff,
                bits & 0xffff,
            )
        }
        Err(error) => {
            crcbl_core::log::debug!(
                "crcbl-dx12: no user-mode driver version for this adapter: {error}"
            );
            "D3D12 (no user-mode driver version)".to_string()
        }
    }
}

/// The adapter's name, from the fixed-width UTF-16 field DXGI fills in.
///
/// `DXGI_ADAPTER_DESC1::Description` is a fixed array with the string
/// NUL-terminated inside it, so the terminator is the length and the tail is
/// uninitialised padding. `from_utf16_lossy` is the standard library's answer to
/// the rest; a lone surrogate from a driver becomes a replacement character
/// instead of a failure, which is right for a field that only ever reaches a log.
fn name_of(desc: &DXGI_ADAPTER_DESC1) -> String {
    let end = desc
        .Description
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(desc.Description.len());
    String::from_utf16_lossy(&desc.Description[..end])
}

/// Whether this adapter is a software rasteriser.
///
/// One function rather than the test written twice, because it is asked in two
/// places for two different reasons — `crcbl_dx12::instance` skips software
/// adapters in the ordinary enumeration so that WARP is listed exactly once, and
/// [`describe`] records the answer so [`device_type_of`] can reach it — and two
/// copies of it is precisely where the two would drift.
///
/// # The flag alone is wrong, and the runner is the evidence
///
/// `DXGI_ADAPTER_FLAG_SOFTWARE` is the obvious signal and it is **not
/// sufficient**: `windows-latest` lists "Microsoft Basic Render Driver", which
/// *is* WARP, with that flag clear. Consulting only the flag therefore made
/// [`device_type_of`] answer [`DeviceType::Integrated`] for a software
/// rasteriser — WARP shares the CPU's memory pool, so `UMA` is true for it — and
/// a caller ranking `Discrete > Integrated > Cpu` to prefer hardware would pick
/// it and believe it had a GPU.
///
/// So the known ids are consulted alongside the flag. They are a *known pair*
/// rather than a guarantee, which is why the flag is still tested first: a
/// future software adapter that sets the flag and reports different ids is
/// caught by the flag, and this one is caught by the ids.
/// [`MICROSOFT_VENDOR_ID`] and [`BASIC_RENDER_DRIVER_DEVICE_ID`] both carry the
/// value the runner reported, and both halves are required — Microsoft's vendor
/// id alone would also claim a future hardware adapter Microsoft ships.
///
/// `DXGI_ADAPTER_FLAG_NONE` is deliberately never tested against: it is
/// **zero**, so `Flags & NONE != 0` is false for every adapter that ever existed
/// and `Flags & NONE == 0` is true for every one — a test that cannot fail
/// either way.
pub(crate) fn is_software(desc: &DXGI_ADAPTER_DESC1) -> bool {
    desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0.cast_unsigned() != 0
        || (desc.VendorId == MICROSOFT_VENDOR_ID && desc.DeviceId == BASIC_RENDER_DRIVER_DEVICE_ID)
}

/// Which class of device this is, from what DXGI and D3D12 actually report.
///
/// The order is deliberate: *software or not* settles the question before
/// *where the memory is* does, because WARP shares the CPU's memory pool and
/// would otherwise report as integrated.
///
/// * [`DeviceType::Cpu`] ← [`is_software`], which is the flag **and** the known
///   Microsoft software ids, because the flag is clear on the very rasteriser it
///   is supposed to name. This is WARP, which the seam's own documentation names
///   as an example of the variant.
/// * [`DeviceType::Integrated`] ← `D3D12_FEATURE_DATA_ARCHITECTURE1::UMA`. The
///   real question, rather than the `DedicatedVideoMemory == 0` heuristic that
///   is usually substituted for it.
/// * [`DeviceType::Discrete`] otherwise.
///
/// [`DeviceType::Virtual`] is never reported, and that is a gap rather than a
/// decision: **DXGI has no virtualisation query**. A GPU-P adapter under Hyper-V
/// answers every question above exactly as the host's does, and nothing short of
/// matching on the adapter's name would separate them.
///
/// A device whose `ARCHITECTURE1` query is refused defaults to `UMA` false and so
/// reports as [`DeviceType::Discrete`]. That is the safe direction of the two:
/// nothing in the engine treats a discrete GPU as having the CPU's memory, while
/// the reverse mistake would suggest an upload path that does not exist.
fn device_type_of(raw: &RawCaps) -> DeviceType {
    if raw.software {
        DeviceType::Cpu
    } else if raw.unified_memory {
        DeviceType::Integrated
    } else {
        DeviceType::Discrete
    }
}

/// The seam features this adapter can answer for, and only those.
///
/// # Reported
///
/// * [`Features::DESCRIPTOR_INDEXING`] ← [`RawCaps::dynamic_resources`], which
///   is `ResourceBindingTier` 3 **and** `HighestShaderModel` 6.6 or better, both
///   from real `CheckFeatureSupport` calls. This is the flag
///   `docs/plan/09-backends-metal-dx12.md`'s mapping table calls a near-direct
///   fit and the one `docs/backlog.md` wants measured on WARP.
///
///   **It is reported ahead of the call that will back it, and that is a
///   deliberate exception.** `crcbl-mtl` reported this flag from
///   `argumentBuffersSupport` in its first slice and had to *withdraw* it when
///   its bind groups turned out to bind flat argument tables with no
///   runtime-sized array. The reason the same reversal is not a lie here is that
///   nothing a caller can reach acts on the flag: the device slice creates no
///   bind group layout and no pipeline, so there is nothing to be misled. **The
///   slice that builds bind groups owns this flag** — if
///   `create_bind_group_layout` cannot honour
///   [`BindingFlags`](crcbl_hal::BindingFlags) on a descriptor heap, it must
///   come off, exactly as it came off Metal.
/// * [`Features::BUFFER_DEVICE_ADDRESS`] — **unconditional, and there is no
///   query to make.** Every D3D12 buffer resource answers
///   `ID3D12Resource::GetGPUVirtualAddress`, and a root SRV/UAV/CBV descriptor
///   *is* a raw GPU virtual address; there is no capability bit for it because
///   there is no D3D12 device without it. The plan's mapping table calls it a
///   direct fit for the same reason. Reported with a comment rather than with a
///   query that would only ever answer yes.
/// * [`Features::TEXTURE_COMPRESSION_BC`] ←
///   [`supports_block_compression`], which asks
///   `CheckFeatureSupport(D3D12_FEATURE_FORMAT_SUPPORT)` about every BC format
///   the seam declares and requires both `TEXTURE2D` and `SHADER_SAMPLE` on
///   each. The query needs a `DXGI_FORMAT` and so had to wait for the format
///   table [`crate::conv`] brought; `Device::create_image` creates images from
///   it, which is what earns the flag rather than merely enabling the question.
/// * [`Features::SAMPLER_ANISOTROPY`] — unconditional in D3D12, which states
///   `D3D12_REQ_MAXANISOTROPY` as an architectural constant rather than a
///   per-device answer. Reported now that `Device::create_sampler` builds a
///   `D3D12_SAMPLER_DESC` with a `MaxAnisotropy` in it; `crcbl-mtl` withheld it
///   until exactly the same point and for the same reason.
/// * [`Features::COMPUTE`] — **unconditional, and there is no query to make.**
///   Every D3D12 device creates a `DIRECT` queue, and a `DIRECT` queue accepts
///   compute work; there is no D3D12 device without
///   `CreateComputePipelineState`, which is why the D3D12 capability structures
///   have no bit for it. It waited for the call rather than for a query:
///   `Device::create_compute_pipeline` builds the state object,
///   `CommandEncoder::dispatch` and `dispatch_indirect` record against it, and
///   `crates/crcbl/tests/hal_seam_e2e.rs`'s
///   `a_compute_dispatch_writes_the_values_it_was_asked_for` reads the result
///   back on WARP, as it does on every other backend. Reporting it before that
///   would have been the
///   "unsupported arriving as passed" shape the crate docs name.
/// * [`Features::DRAW_INDIRECT_COUNT`], [`Features::MULTI_DRAW_INDIRECT`] and
///   [`Features::INDIRECT_FIRST_INSTANCE`] — **unconditional, and there is no
///   query to make.** All three are parameters and fields of one call rather
///   than capability bits: `ExecuteIndirect` takes a `MaxCommandCount` (which is
///   multi-draw), an optional `pCountBuffer` (which is the GPU-side count), and
///   reads `StartInstanceLocation` out of `D3D12_DRAW_ARGUMENTS` like every
///   other field of it. There is no D3D12 device without `ExecuteIndirect`,
///   which is why the capability structures have no bit for any of them.
///
///   They waited for the calls rather than for a query, exactly as
///   [`Features::COMPUTE`] did: `CommandEncoder::draw_indirect`,
///   `draw_indexed_indirect` and both `_count` siblings record against the
///   command signatures `crate::draw` describes, and `crcbl_dx12::device`'s
///   indirect draw tests read the picture back. Reporting them earlier would
///   have been the "unsupported arriving as passed" shape the crate docs name —
///   and `crcbl-mtl` withdrawing a flag no call had earned is the precedent that
///   made the wait the rule.
///
///   **`DRAW_INDIRECT_COUNT` is the one that moves a selector.** With it
///   reported and no mesh shader, [`GeometryPath::from_features`](crcbl_hal::GeometryPath::from_features)
///   puts this backend on
///   [`IndirectCount`](crcbl_hal::GeometryPath::IndirectCount) rather than the
///   per-batch floor — the arm `crcbl-mtl` cannot reach, because Metal has no
///   count-from-memory execution to back it.
/// * [`Features::PRESENT_FEEDBACK`] — **unconditional, and for a stronger
///   reason than Metal's.** The mechanism is
///   `DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT` plus
///   `IDXGISwapChain2::GetFrameLatencyWaitableObject`, and the flag is a
///   **swapchain creation** flag rather than a device property — so the
///   question this raises is the one `crcbl-mtl` had to argue: is a
///   device-level flag honest about a per-swapchain capability?
///
///   Here it is, and the argument does not rest on the same "no honest
///   device-level answer" trade Metal made. `IDXGISwapChain2` arrived with DXGI
///   1.3 in Windows 8.1 and D3D12 requires Windows 10, so **every machine that
///   can open an `ID3D12Device` at all can create a waitable swapchain** —
///   there is no query to make, no driver to ask, and no machine where this
///   would have been probed and come back no. `crcbl-vk` cannot say that:
///   `VK_KHR_present_wait` is driver-conditional and its adapter probes for it.
///   And `crcbl_dx12::swapchain` sets the flag on **every** swapchain it
///   creates, so there is no swapchain on this backend for which the claim is
///   false.
///
///   The flag is also read once, at device open, before any swapchain exists —
///   `GpuContext::open` logs which pacing story a run gets from it — which is
///   the structural reason a per-swapchain fact has to be answered at device
///   level whatever the backend.
/// * [`Features::TIMELINE_SEMAPHORE`] — **unconditional, and there is no query
///   to make.** `ID3D12Fence` is a monotonic `u64` counter with no separate
///   binary form, and `CreateFence` is not an optional device capability: there
///   is no D3D12 device without it, which is why the capability structures have
///   no bit for it.
///
///   It waited for the calls rather than for a query, exactly as
///   [`Features::COMPUTE`] did. `Device::create_semaphore` hands an
///   `ID3D12Fence` out for both [`SemaphoreKind`](crcbl_hal::SemaphoreKind)s,
///   `Device::submit` consumes it through `ID3D12CommandQueue::Wait` and
///   `Signal`, and `Device::semaphore_value` and `Device::wait_semaphores` are
///   the CPU half — `GetCompletedValue` and a `SetEventOnCompletion` armed with
///   a deadline. `crates/crcbl/tests/hal_seam_e2e.rs`'s
///   `a_timeline_semaphore_signals_from_a_submission_and_the_cpu_sees_it`
///   drives the whole sequence on WARP, as it does on every other backend.
///
///   **This flag completes [`Features::GPU_DRIVEN`] on this backend**, so an
///   adapter reporting [`Features::DESCRIPTOR_INDEXING`] now reports the whole
///   named union and [`DeviceDesc::for_adapter`](crcbl_hal::DeviceDesc::for_adapter)
///   opens rather than refusing.
///
/// * [`Features::TIMESTAMP_QUERY`], [`Features::OCCLUSION_QUERY`] and
///   [`Features::PIPELINE_STATISTICS_QUERY`] — **unconditional, and there is no
///   query to make.** `CreateQueryHeap` accepts
///   `D3D12_QUERY_HEAP_TYPE_TIMESTAMP`, `_OCCLUSION` and
///   `_PIPELINE_STATISTICS` on every D3D12 device, and a `DIRECT` queue takes
///   all three: the one query heap type that *is* conditional is
///   `_COPY_QUEUE_TIMESTAMP`, gated on
///   `D3D12_FEATURE_DATA_D3D12_OPTIONS3::CopyQueueTimestampQueriesSupported`,
///   and this backend creates no copy queue to put one on. That is why the
///   capability structures have no bit for the three that are here.
///
///   They waited for the calls rather than for a query, exactly as
///   [`Features::COMPUTE`] did. `Device::create_query_set` creates the heap and
///   the resolve destination behind it, `RenderPassDesc::timestamp_writes` and
///   `CommandEncoder::resolve_query_set` record `EndQuery` and
///   `ResolveQueryData`, and
///   `Device::query_results` reads a set back.
///
///   **The clock's rate is the half an adapter cannot answer**, because
///   `ID3D12CommandQueue::GetTimestampFrequency` needs a queue and there is
///   none here. It never reaches the seam: `Device::query_results` reports
///   nanoseconds and `Dx12Device::open` reads the frequency once, for its own
///   use, so an adapter and a device from it report identical [`Limits`]. Only
///   the flag is reported here, and it has to be:
///   [`DeviceDesc::required_features`](crcbl_hal::DeviceDesc::required_features)
///   is checked against the **adapter**, so a flag that first appeared at device
///   open could never be required by a caller.
///
/// * [`Features::PUSH_CONSTANTS`] — **unconditional, and there is no query to
///   make.** Root constants are a root parameter type, not a capability:
///   `D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS` is accepted by every root
///   signature D3D12 serialises, which is why the capability structures have no
///   bit for it.
///
///   It waited for the calls rather than for a query, exactly as
///   [`Features::COMPUTE`] did. `Device::create_pipeline_layout` builds the
///   parameter — at the `b` register `crcbl-shaders`' committed DXIL puts the
///   block at, which `crcbl_dx12::root` derives rather than assumes — and
///   `CommandEncoder::push_constants` sets it with
///   `SetGraphicsRoot32BitConstants` or its compute twin.
///
///   **[`Limits::max_push_constant_size`] is the whole root signature**, and
///   that is a ceiling rather than a promise: a root constant costs one of the
///   64 DWORDs per 32-bit value, so a range using all of it leaves room for no
///   descriptor table at all. `crcbl_dx12::root`'s `place` refuses that
///   combination at pipeline-layout creation with the arithmetic in the
///   message, which is the honest shape — a smaller figure here would refuse
///   ranges D3D12 accepts, and the seam's limit has no way to say "shared with
///   your bind groups".
///
/// # Absent, with the reason for each
///
/// * [`Features::DEBUG_MARKERS`] — PIX events go through `WinPixEventRuntime`,
///   a library this workspace does not depend on, so this needs a decision
///   before it needs a slice.
/// * [`Features::ASYNC_COMPUTE_QUEUE`] and [`Features::TRANSFER_QUEUE`] —
///   `D3D12_COMMAND_LIST_TYPE_COMPUTE` and `_COPY` are exactly these, and
///   `crcbl_hal::QueueKind` already records that as why it is not named
///   `QueueFamily`. The device slice creates only the direct queue, because a
///   queue handed out for a feature that is not reported is this rule in
///   reverse; the slice that has work to put on a second queue reports it.
/// * [`Features::SHADER_DEBUG_PRINTF`] — the debug layer's GPU-based validation
///   is the closest thing, and nothing routes it into `log` yet.
fn features_of(raw: &RawCaps) -> Features {
    let mut out = Features::empty();
    if raw.dynamic_resources() {
        out |= Features::DESCRIPTOR_INDEXING;
    }
    if raw.block_compression {
        out |= Features::TEXTURE_COMPRESSION_BC;
    }
    // No query for any of these: a GPU virtual address is not optional in
    // D3D12, anisotropic sampling is an architectural constant, compute is what
    // a DIRECT queue accepts by definition, the three indirect flags are
    // parameters and fields of `ExecuteIndirect` rather than capability bits,
    // the frame-latency waitable object predates D3D12 itself, `CreateFence` is
    // on every D3D12 device there is, so are the three query heap types a
    // DIRECT queue takes, and root constants are a root parameter type rather
    // than a capability. See above.
    //
    // The three rasteriser flags are here because CI caught them missing: the
    // seam suite drew a wireframe and a primitive past the far plane on WARP,
    // both worked, and the report said this backend "declares it unsupported
    // and then performed it". They had been withheld on the grounds that
    // `D3D12_RASTERIZER_DESC` is filled in by the pipeline slice — which landed,
    // taking the reason with it. `FillMode`, `DepthClipEnable` and
    // `DepthBiasClamp` are plain fields of that struct with no capability bit
    // behind them, so on D3D12 there is nothing to query and nothing to gate.
    out |= Features::BUFFER_DEVICE_ADDRESS
        | Features::DEPTH_CLAMP
        | Features::DEPTH_BIAS_CLAMP
        | Features::POLYGON_MODE_LINE
        | Features::SAMPLER_ANISOTROPY
        | Features::COMPUTE
        | Features::DRAW_INDIRECT_COUNT
        | Features::MULTI_DRAW_INDIRECT
        | Features::INDIRECT_FIRST_INSTANCE
        | Features::PRESENT_FEEDBACK
        | Features::PUSH_CONSTANTS
        | Features::TIMELINE_SEMAPHORE
        | Features::TIMESTAMP_QUERY
        | Features::OCCLUSION_QUERY
        | Features::PIPELINE_STATISTICS_QUERY;
    out
}

/// This adapter's numeric ceilings.
///
/// D3D12 states most of these as **architectural constants** rather than as
/// per-device answers — they are the numbers a device must meet to be a D3D12
/// device at all — so the shape is "start at [`Limits::minimum`] and replace
/// what D3D12 genuinely specifies". Every field left at the floor is a
/// **contract this backend guarantees**, not a measurement: the seam says a
/// limit is a hard ceiling, and a ceiling lower than the truth is always safe
/// while one that is higher is a crash on someone else's machine.
///
/// Taken from D3D12's own constants:
///
/// * [`D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION`],
///   [`D3D12_REQ_TEXTURE3D_U_V_OR_W_DIMENSION`] and
///   [`D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION`] → the image limits.
/// * [`D3D12_REQ_CONSTANT_BUFFER_ELEMENT_COUNT`] ×
///   [`CONSTANT_BUFFER_ELEMENT_BYTES`] → the uniform-buffer range.
/// * [`D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT`] → the colour-attachment count.
/// * [`D3D12_CS_THREAD_GROUP_MAX_X`] / `_Y` / `_Z`,
///   [`D3D12_CS_THREAD_GROUP_MAX_THREADS_PER_GROUP`] and
///   [`D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION`] → the compute
///   limits, which D3D12 fixes for every device rather than reporting per
///   device the way Vulkan and Metal do.
/// * [`D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT`],
///   [`D3D12_RAW_UAV_SRV_BYTE_ALIGNMENT`] and
///   [`D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT`] → the three alignments.
///
/// Left at the floor, each for a stated reason:
///
/// * `max_storage_buffer_range` — D3D12's cap is
///   `D3D12_REQ_BUFFER_RESOURCE_TEXEL_COUNT_2_TO_EXP`, an exponent for a **texel
///   count**. The byte range it implies depends on the view's element stride,
///   which no adapter query knows, so there is no honest byte figure to report
///   and the floor stands.
/// * `max_bind_groups` — **not a number D3D12 has.** `D3D12_MAX_ROOT_COST`
///   bounds the whole signature in DWORDs, and what a set spends out of it
///   depends on what the set holds: a descriptor table costs one, a root
///   descriptor two, and a push-constant range one per 32-bit value. So four
///   sets fit or they do not depending on their contents, and any fixed count
///   reported here would be a promise `crcbl_dx12::root::place` can refuse.
///   The floor is what can always be served whatever the sets hold, and an
///   overrun is refused at `create_pipeline_layout` with the arithmetic on both
///   sides rather than being pre-empted by a smaller limit that would also
///   refuse layouts D3D12 accepts.
///
///   (This bullet used to say the pipeline-layout slice would decide it. That
///   slice landed — `place` is what does the deciding, and it decides per
///   layout rather than once.)
/// * `max_sample_count` — `CheckFeatureSupport(D3D12_FEATURE_MULTISAMPLE_QUALITY_LEVELS)`
///   answers it **per format**, so it needs a format table. The floor is
///   WebGPU's downlevel guarantee and D3D12 clears it everywhere.
///
/// The feature-keyed fields do move:
///
/// * `max_bindless_descriptors` moves to
///   [`D3D12_MAX_SHADER_VISIBLE_DESCRIPTOR_HEAP_SIZE_TIER_2`] deliberately:
///   [`Features::DESCRIPTOR_INDEXING`] requires binding tier 3, for which D3D12
///   defines **no constant at all** because the heap is bounded only by memory.
///   The tier 2 constant is the largest figure the API commits to in writing,
///   which makes it a ceiling this backend can actually honour.
/// * `max_sampler_anisotropy` moves to [`D3D12_REQ_MAXANISOTROPY`], the value
///   `D3D12_SAMPLER_DESC::MaxAnisotropy` is documented as capped at, now that
///   [`Features::SAMPLER_ANISOTROPY`] is reported.
/// * `max_draw_indirect_count` moves to `u32::MAX` now that
///   [`Features::DRAW_INDIRECT_COUNT`] is, and that really is the ceiling:
///   D3D12 states no `D3D12_REQ_*` constant for it because the number is
///   `ExecuteIndirect`'s own `MaxCommandCount` parameter, a `UINT`. So the
///   API's ceiling is the type's, and a lower figure here would be this backend
///   refusing calls D3D12 accepts.
/// * `max_push_constant_size` moves to
///   [`root::MAX_PUSH_CONSTANT_BYTES`](crate::root::MAX_PUSH_CONSTANT_BYTES)
///   now that [`Features::PUSH_CONSTANTS`] is — the whole root signature in
///   bytes, for the reason given with the flag above.
fn limits_of(features: Features) -> Limits {
    let floor = Limits::minimum();
    Limits {
        max_image_2d: D3D12_REQ_TEXTURE2D_U_OR_V_DIMENSION,
        max_image_3d: D3D12_REQ_TEXTURE3D_U_V_OR_W_DIMENSION,
        max_image_array_layers: D3D12_REQ_TEXTURE2D_ARRAY_AXIS_DIMENSION,
        max_uniform_buffer_range: u64::from(D3D12_REQ_CONSTANT_BUFFER_ELEMENT_COUNT)
            * CONSTANT_BUFFER_ELEMENT_BYTES,
        // The heap this backend actually allocates, **not**
        // `D3D12_MAX_SHADER_VISIBLE_DESCRIPTOR_HEAP_SIZE_TIER_2`. That constant
        // is a million and was reported here, which is roughly 244x what
        // `crate::binding` can ever serve: it allocates one shader-visible view
        // heap of `VIEW_DESCRIPTORS` up front and in full. A caller believing
        // the million got `OutOfDeviceMemory` at `create_bind_group` — not the
        // `InvalidDescriptor` `Limits` documents for exceeding a ceiling, and
        // dependent on what else was live at the time. A limit is a promise the
        // backend keeps, so it has to be the number it can keep.
        max_bindless_descriptors: if features.contains(Features::DESCRIPTOR_INDEXING) {
            crate::binding::VIEW_DESCRIPTORS
        } else {
            floor.max_bindless_descriptors
        },
        max_sampler_anisotropy: if features.contains(Features::SAMPLER_ANISOTROPY) {
            #[allow(clippy::cast_precision_loss)]
            {
                D3D12_REQ_MAXANISOTROPY as f32
            }
        } else {
            floor.max_sampler_anisotropy
        },
        max_color_attachments: D3D12_SIMULTANEOUS_RENDER_TARGET_COUNT,
        max_draw_indirect_count: if features.contains(Features::DRAW_INDIRECT_COUNT) {
            u32::MAX
        } else {
            floor.max_draw_indirect_count
        },
        max_push_constant_size: if features.contains(Features::PUSH_CONSTANTS) {
            crate::root::MAX_PUSH_CONSTANT_BYTES
        } else {
            floor.max_push_constant_size
        },
        max_compute_workgroup_size: [
            D3D12_CS_THREAD_GROUP_MAX_X,
            D3D12_CS_THREAD_GROUP_MAX_Y,
            D3D12_CS_THREAD_GROUP_MAX_Z,
        ],
        max_compute_invocations_per_workgroup: D3D12_CS_THREAD_GROUP_MAX_THREADS_PER_GROUP,
        max_compute_workgroups_per_dimension: D3D12_CS_DISPATCH_MAX_THREAD_GROUPS_PER_DIMENSION,
        min_uniform_buffer_offset_alignment: u64::from(
            D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT,
        ),
        min_storage_buffer_offset_alignment: u64::from(D3D12_RAW_UAV_SRV_BYTE_ALIGNMENT),
        optimal_buffer_copy_offset_alignment: u64::from(D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT),
        ..floor
    }
}

/// Everything one D3D12 adapter says about itself, or `None` if D3D12 will not
/// open it.
///
/// The `ID3D12Device` created here exists only for the duration of the call. It
/// is created at [`D3D_FEATURE_LEVEL_11_0`] — D3D12's floor — on purpose:
/// asking for a higher level would silently drop adapters from the enumeration,
/// and which tier a device can run is decided by [`Features`], never by the
/// feature level it was opened at.
pub(crate) fn describe(
    id: AdapterId,
    adapter: &IDXGIAdapter1,
    desc: &DXGI_ADAPTER_DESC1,
) -> Option<(AdapterInfo, RawCaps)> {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: `adapter` is a live `IDXGIAdapter1` this crate owns a reference
    // to, and `device` is a live `Option<ID3D12Device>` the call writes through.
    // A failure leaves it `None`, which is why it is read back rather than
    // assumed.
    let created = unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) };
    if let Err(error) = created {
        crcbl_core::log::debug!(
            "crcbl-dx12: DXGI lists \"{}\" but D3D12 will not open it: {error}",
            name_of(desc)
        );
        return None;
    }
    // A successful `D3D12CreateDevice` always wrote an interface, so this is not
    // a case anyone expects — but the binding's out-parameter is an `Option` and
    // a `None` here would be a null pointer to call through, so it is read back
    // rather than unwrapped.
    let device = device?;

    // **A refused query degrades to "no", never to a false "yes".**
    // `D3D12_FEATURE_DATA_D3D12_OPTIONS::default()` leaves `ResourceBindingTier`
    // at zero, which is below `D3D12_RESOURCE_BINDING_TIER_1` and therefore below
    // the tier `RawCaps::dynamic_resources` requires — so a driver that will not
    // answer loses bindless rather than gaining it. That is the direction a
    // limit or a feature must fail in.
    let options =
        check_feature(&device, D3D12_FEATURE_DATA_D3D12_OPTIONS::default()).unwrap_or_default();
    // `NodeIndex` is the in half of this in/out query. Zero is the only node the
    // seam models: `crcbl-hal` has no multi-adapter vocabulary at all, so a
    // linked-node rig is described here as its first node rather than wrongly.
    let architecture = check_feature(
        &device,
        D3D12_FEATURE_DATA_ARCHITECTURE1 {
            NodeIndex: 0,
            ..Default::default()
        },
    )
    .unwrap_or_default();

    let raw = RawCaps {
        binding_tier: options.ResourceBindingTier,
        shader_model: highest_shader_model(&device),
        software: is_software(desc),
        unified_memory: architecture.UMA.as_bool(),
        // The two halves are distinct fields rather than one integer in the
        // API, so they are packed here in a fixed order; nothing reads the
        // parts back, only compares whole values.
        luid: (u64::from(desc.AdapterLuid.HighPart.cast_unsigned()) << 32)
            | u64::from(desc.AdapterLuid.LowPart),
        block_compression: supports_block_compression(&device),
    };
    let features = features_of(&raw);
    let info = AdapterInfo {
        id,
        name: name_of(desc),
        // Unlike Metal, DXGI reports genuine PCI ids, so these are the real
        // thing rather than the seam's `0` for "unknown". WARP reports
        // Microsoft's own vendor id here, which is what makes it identifiable in
        // a log without matching on its name.
        vendor_id: desc.VendorId,
        device_id: desc.DeviceId,
        device_type: device_type_of(&raw),
        driver: driver_string(adapter),
        backend: BackendKind::Dx12,
        caps: DeviceCaps {
            features,
            limits: limits_of(features),
        },
    };
    Some((info, raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// [`D3D12_RESOURCE_BINDING_TIER_2`] is only needed by the tests — the
    /// backend itself only ever compares against tier 3. Everything else here is
    /// the mesh probe's, and named where that test says what each answer
    /// settles.
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_FEATURE_D3D12_OPTIONS3, D3D12_FEATURE_D3D12_OPTIONS7,
        D3D12_FEATURE_DATA_D3D12_OPTIONS3, D3D12_FEATURE_DATA_D3D12_OPTIONS7,
        D3D12_MESH_SHADER_TIER, D3D12_MESH_SHADER_TIER_1, D3D12_MESH_SHADER_TIER_NOT_SUPPORTED,
        D3D12_RESOURCE_BINDING_TIER_2, D3D12_SAMPLER_FEEDBACK_TIER,
        D3D12_SAMPLER_FEEDBACK_TIER_0_9, D3D12_SAMPLER_FEEDBACK_TIER_1_0,
        D3D12_SAMPLER_FEEDBACK_TIER_NOT_SUPPORTED,
    };

    use crate::device::tests::open_device;
    use crate::instance::tests::pinned_adapter;

    /// The nibble decoding really is the encoding the constants use.
    ///
    /// [`shader_model_name`] is what puts `"6.6"` in the line this slice exists
    /// to publish, so a decoding bug would misreport the exact number
    /// `docs/backlog.md` is waiting for. Checked against the bindings' own
    /// constants rather than against the prose on the function, and against the
    /// two ends of the range as well as the middle — a decoder that dropped the
    /// shift would pass on `6.6` alone, since both nibbles are the same digit.
    #[test]
    fn the_shader_model_decoding_matches_the_constants_it_decodes() {
        assert_eq!(shader_model_name(D3D_SHADER_MODEL_5_1), "5.1");
        assert_eq!(shader_model_name(D3D_SHADER_MODEL_6_0), "6.0");
        assert_eq!(shader_model_name(D3D_SHADER_MODEL_6_6), "6.6");
        assert_eq!(shader_model_name(D3D_SHADER_MODEL_6_9), "6.9");
        assert_eq!(shader_model_name(D3D_SHADER_MODEL_NONE), "none");
    }

    /// The probe list is ordered newest-first and includes the model the whole
    /// slice turns on.
    ///
    /// [`highest_shader_model`] returns the **first** probe that succeeds, so an
    /// out-of-order entry would report a lower model than the device supports on
    /// every machine — and silently, because a lower model is a plausible
    /// answer. A missing [`D3D_SHADER_MODEL_6_6`] would make
    /// [`RawCaps::dynamic_resources`] unable to ever answer yes on hardware
    /// whose ceiling is exactly 6.6.
    #[test]
    fn the_probed_shader_models_descend_and_contain_the_one_that_matters() {
        assert!(
            PROBED_SHADER_MODELS.contains(&D3D_SHADER_MODEL_6_6),
            "the model dynamic resources needs is not probed"
        );
        for pair in PROBED_SHADER_MODELS.windows(2) {
            assert!(
                pair[0].0 > pair[1].0,
                "{:?} does not come before {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    /// Dynamic resources need **both** answers, and neither one alone.
    ///
    /// The failing shapes this pins are the two easy bugs: a rule that checks
    /// only the shader model reports bindless on tier 2 hardware, and one that
    /// checks only the binding tier reports it on a device whose compiler cannot
    /// spell `ResourceDescriptorHeap`. Both would put `DESCRIPTOR_INDEXING` on
    /// an adapter that cannot honour it — and both would change the answer this
    /// slice publishes about WARP.
    #[test]
    fn dynamic_resources_need_the_binding_tier_and_the_shader_model_together() {
        let base = RawCaps {
            binding_tier: D3D12_RESOURCE_BINDING_TIER_3,
            shader_model: D3D_SHADER_MODEL_6_6,
            software: false,
            unified_memory: false,
            luid: 0,
            block_compression: true,
        };
        assert!(base.dynamic_resources(), "tier 3 with SM6.6 is the answer");

        let older_model = RawCaps {
            shader_model: D3D_SHADER_MODEL_6_5,
            ..base
        };
        assert!(
            !older_model.dynamic_resources(),
            "tier 3 alone is not enough: {older_model:?}"
        );

        let lower_tier = RawCaps {
            binding_tier: D3D12_RESOURCE_BINDING_TIER_2,
            ..base
        };
        assert!(
            !lower_tier.dynamic_resources(),
            "SM6.6 alone is not enough: {lower_tier:?}"
        );

        // A device newer than the constant this rule names must still answer
        // yes — a rule written with `==` instead of `>=` would fail here and
        // nowhere else, and would take bindless away from every future GPU.
        let newer = RawCaps {
            shader_model: D3D_SHADER_MODEL_6_9,
            ..base
        };
        assert!(newer.dynamic_resources(), "{newer:?}");
    }

    /// The features and the limits keyed off them cannot disagree.
    ///
    /// Checked on constructed [`RawCaps`] rather than on a real adapter, because
    /// the interesting case — bindless present *and* absent — does not exist on
    /// one machine. The falsifying value is a bindless capacity reported for an
    /// adapter without the bindless feature, which is the promise `crcbl-wgpu`
    /// learnt the hard way that no call can keep.
    #[test]
    fn a_bindless_capacity_is_reported_exactly_when_bindless_is() {
        let floor = Limits::minimum();
        for (binding_tier, shader_model) in [
            (D3D12_RESOURCE_BINDING_TIER_3, D3D_SHADER_MODEL_6_6),
            (D3D12_RESOURCE_BINDING_TIER_3, D3D_SHADER_MODEL_6_0),
        ] {
            let raw = RawCaps {
                binding_tier,
                shader_model,
                software: false,
                unified_memory: false,
                luid: 0,
                block_compression: true,
            };
            let features = features_of(&raw);
            let limits = limits_of(features);
            assert_eq!(
                features.contains(Features::DESCRIPTOR_INDEXING),
                limits.max_bindless_descriptors > floor.max_bindless_descriptors,
                "bindless capacity and the bindless feature disagree: {raw:?}"
            );
            assert!(
                features.contains(Features::BUFFER_DEVICE_ADDRESS),
                "every D3D12 device has GPU virtual addresses: {raw:?}"
            );
            // Root constants are a root parameter type rather than a capability,
            // so this holds for the lesser adapter as well as the tier-3 one —
            // and the budget must move with the flag whichever way it is
            // reported, which is the pairing `crcbl_dx12::instance`'s
            // `reported_limits_come_from_d3d12_and_agree_with_the_features`
            // asserts against a real device and this one asserts without one.
            assert!(
                features.contains(Features::PUSH_CONSTANTS),
                "root constants are on every D3D12 device: {raw:?}"
            );
            assert_eq!(
                limits.max_push_constant_size,
                crate::root::MAX_PUSH_CONSTANT_BYTES,
                "the reported budget is the whole root signature, which is what \
                 crcbl_dx12::root::place spends it from: {raw:?}"
            );
            assert!(
                limits.max_push_constant_size > floor.max_push_constant_size,
                "a floor here would be this backend refusing ranges D3D12 takes: {raw:?}"
            );
        }
    }

    /// **The software test does not depend on the flag being set**, because on
    /// the runner it is not.
    ///
    /// Written on constructed descriptors, because the combination that matters
    /// — Microsoft's software ids with `DXGI_ADAPTER_FLAG_SOFTWARE` *clear* — is
    /// what `windows-latest` produces and what no machine with a real GPU can be
    /// relied on to produce. The hardware cases are asserted in the same test:
    /// without them a rule that answered "software" to everything would pass,
    /// and every adapter on every machine would report as [`DeviceType::Cpu`].
    ///
    /// The falsifying edit is dropping either half of the id pair from
    /// [`is_software`] — the vendor alone claims any future Microsoft hardware,
    /// and the device id alone claims another vendor's part that happens to
    /// share the number.
    #[test]
    fn the_software_test_reads_the_known_ids_as_well_as_the_flag() {
        let flagged = DXGI_ADAPTER_FLAG_SOFTWARE.0.cast_unsigned();
        let warp_ids = DXGI_ADAPTER_DESC1 {
            VendorId: MICROSOFT_VENDOR_ID,
            DeviceId: BASIC_RENDER_DRIVER_DEVICE_ID,
            ..Default::default()
        };
        assert!(
            is_software(&warp_ids),
            "the Basic Render Driver arrives with the flag clear and must still be software: \
             vendor={:#06x} device={:#06x} flags={:#x}",
            warp_ids.VendorId,
            warp_ids.DeviceId,
            warp_ids.Flags
        );
        let warp_flagged = DXGI_ADAPTER_DESC1 {
            Flags: flagged,
            ..warp_ids
        };
        assert!(is_software(&warp_flagged), "{:#x}", warp_flagged.Flags);

        // Both halves of the pair are required, and each alone is somebody
        // else's adapter.
        let other_vendor = DXGI_ADAPTER_DESC1 {
            VendorId: MICROSOFT_VENDOR_ID + 1,
            ..warp_ids
        };
        assert!(
            !is_software(&other_vendor),
            "another vendor's part sharing the device id is not WARP: vendor={:#06x} \
             device={:#06x}",
            other_vendor.VendorId,
            other_vendor.DeviceId
        );
        let other_device = DXGI_ADAPTER_DESC1 {
            DeviceId: BASIC_RENDER_DRIVER_DEVICE_ID + 1,
            ..warp_ids
        };
        assert!(
            !is_software(&other_device),
            "a different Microsoft part is not the Basic Render Driver: vendor={:#06x} \
             device={:#06x}",
            other_device.VendorId,
            other_device.DeviceId
        );

        // A real GPU is hardware, and the flag still answers for a software
        // adapter that does set it.
        let hardware = DXGI_ADAPTER_DESC1 {
            VendorId: MICROSOFT_VENDOR_ID + 1,
            DeviceId: BASIC_RENDER_DRIVER_DEVICE_ID + 1,
            ..Default::default()
        };
        assert!(
            !is_software(&hardware),
            "every adapter would report as Cpu: vendor={:#06x} device={:#06x} flags={:#x}",
            hardware.VendorId,
            hardware.DeviceId,
            hardware.Flags
        );
        let flagged_hardware_ids = DXGI_ADAPTER_DESC1 {
            Flags: flagged,
            ..hardware
        };
        assert!(
            is_software(&flagged_hardware_ids),
            "the flag is still consulted: flags={:#x}",
            flagged_hardware_ids.Flags
        );
    }

    /// A software adapter is the seam's `Cpu`, whatever its memory looks like.
    ///
    /// WARP shares the CPU's memory pool, so `UMA` is true for it — a
    /// classification that tested memory first would call the engine's only
    /// Windows software rasteriser an integrated GPU, and every "is this a real
    /// device?" check downstream would believe it.
    #[test]
    fn a_software_adapter_outranks_its_memory_architecture() {
        let warp = RawCaps {
            binding_tier: D3D12_RESOURCE_BINDING_TIER_3,
            shader_model: D3D_SHADER_MODEL_6_6,
            software: true,
            unified_memory: true,
            luid: 0,
            block_compression: true,
        };
        assert_eq!(device_type_of(&warp), DeviceType::Cpu, "{warp:?}");

        let integrated = RawCaps {
            software: false,
            ..warp
        };
        assert_eq!(
            device_type_of(&integrated),
            DeviceType::Integrated,
            "{integrated:?}"
        );

        let discrete = RawCaps {
            unified_memory: false,
            luid: 0,
            ..integrated
        };
        assert_eq!(
            device_type_of(&discrete),
            DeviceType::Discrete,
            "{discrete:?}"
        );
    }

    /// The prefix every line the mesh probe prints carries, so one `grep` over a
    /// CI log pulls the whole measurement out of a suite that prints a great
    /// deal else.
    const MESH_PROBE: &str = "crcbl-dx12 mesh";

    // The two feature ids the probe below adds, each paired with the struct
    // D3D12 fills in for it. They live here rather than beside the production
    // pairings because nothing above the probe asks either question: this
    // backend reports no mesh shading, and its timestamp queries are the graphics
    // and compute queues' rather than the copy queue's. The pairing is an `impl`
    // rather than a hand-written feature id at the call site for the reason
    // `FeatureQuery` exists at all — `CheckFeatureSupport` takes a void pointer,
    // and a mismatched pair is a struct filled in with another feature's fields.
    impl FeatureQuery for D3D12_FEATURE_DATA_D3D12_OPTIONS7 {
        const FEATURE: D3D12_FEATURE = D3D12_FEATURE_D3D12_OPTIONS7;
    }

    impl FeatureQuery for D3D12_FEATURE_DATA_D3D12_OPTIONS3 {
        const FEATURE: D3D12_FEATURE = D3D12_FEATURE_D3D12_OPTIONS3;
    }

    /// A [`D3D12_MESH_SHADER_TIER`] as the name D3D12 gives it.
    ///
    /// The bindings name every tier D3D12 had when this crate's `windows` pin
    /// was cut. A value they do not name is a newer runtime rather than an
    /// error, so it is reported as unnamed instead of being claimed as one of
    /// the named ones — and the raw number is printed beside every one of these
    /// anyway.
    fn mesh_shader_tier_name(tier: D3D12_MESH_SHADER_TIER) -> &'static str {
        if tier == D3D12_MESH_SHADER_TIER_NOT_SUPPORTED {
            "NOT_SUPPORTED"
        } else if tier == D3D12_MESH_SHADER_TIER_1 {
            "TIER_1"
        } else {
            "a tier this windows binding does not name"
        }
    }

    /// A [`D3D12_SAMPLER_FEEDBACK_TIER`] as the name D3D12 gives it, on the same
    /// terms as [`mesh_shader_tier_name`].
    fn sampler_feedback_tier_name(tier: D3D12_SAMPLER_FEEDBACK_TIER) -> &'static str {
        if tier == D3D12_SAMPLER_FEEDBACK_TIER_NOT_SUPPORTED {
            "NOT_SUPPORTED"
        } else if tier == D3D12_SAMPLER_FEEDBACK_TIER_0_9 {
            "TIER_0_9"
        } else if tier == D3D12_SAMPLER_FEEDBACK_TIER_1_0 {
            "TIER_1_0"
        } else {
            "a tier this windows binding does not name"
        }
    }

    /// **The measurement that decides whether this backend's two mesh rows can
    /// ever be _proved_ here** —
    /// [`Capability::MeshShading`](crcbl_hal::Capability::MeshShading) and
    /// [`TaskShaderStage`](crcbl_hal::Capability::TaskShaderStage), which
    /// `crcbl_hal::DIVERGENCES` records for [`BackendKind::Dx12`] as
    /// [`DivergenceKind::Unwritten`](crcbl_hal::DivergenceKind::Unwritten).
    ///
    /// # Why the rows are not what they look like
    ///
    /// `Unwritten` reads as "somebody types the mesh pipeline and the rows go
    /// away", and for this backend that is not established. **This crate has
    /// never asked D3D12 whether it has a mesh shader tier**: [`features_of`]
    /// reports neither [`Features::MESH_SHADER`] nor
    /// [`Features::TASK_SHADER`], and nothing in this crate names
    /// [`D3D12_FEATURE_D3D12_OPTIONS7`] outside this test. So "no mesh shading
    /// on D3D12" is true **by construction rather than by measurement**, and
    /// nobody knows what this crate's CI adapter would answer.
    ///
    /// That matters because both capabilities are gated —
    /// `Capability::gating_feature` maps them onto those two flags — and a gated
    /// capability whose flag is clear is *skipped* by the parity report rather
    /// than checked. A slice that implemented the pipelines and deleted the rows
    /// could therefore leave a green report standing over evidence nobody has.
    ///
    /// It creates nothing, submits nothing and asserts one thing: that the
    /// queries ran. It **prints**, and the CI log is the artifact — the same
    /// shape as `crcbl-mtl`'s
    /// `a_device_reports_its_counter_sampling_gpu_families_and_timestamp_correlation`,
    /// which puts that backend's two unsettled rows' answers into its own log.
    ///
    /// # What each answer settles
    ///
    /// **`MeshShaderTier` together with the shader model.** Neither half is
    /// enough on its own, exactly as [`RawCaps::dynamic_resources`] needs both a
    /// tier and a model:
    ///
    /// * `TIER_1` **and** at least [`D3D_SHADER_MODEL_6_6`] — the two rows are
    ///   ordinary `Unwritten` work, provable on this CI, and the mesh slice is
    ///   worth planning.
    /// * `NOT_SUPPORTED` — the rows can be implemented and **never proved
    ///   here**, because CI has this adapter and no other. That turns the
    ///   deletion question from "finish the work" into "accept an unproven state
    ///   or buy a runner", which is a decision rather than a task.
    /// * A tier with a model below 6.6 — the committed DXIL cannot load whatever
    ///   the hardware can do, because `crcbl-shaders`' manifest ships
    ///   `dxil-model = 6_6`. Different problem, different fix.
    ///
    /// **`SamplerFeedbackTier`** comes back in the same struct and costs nothing
    /// extra, so it is printed rather than discarded.
    ///
    /// **`ResourceBindingTier`** and
    /// **`CopyQueueTimestampQueriesSupported`** are each assumed elsewhere in
    /// this crate — the first by `RawCaps::dynamic_resources`, the second by
    /// there being no copy-queue timestamp path at all — and each is one cheap
    /// call. A log that carries them is a log the next question can be answered
    /// from without another CI round trip.
    ///
    /// # What it fails on, and what it does not
    ///
    /// **A zero is data.** A device honestly answering
    /// `MeshShaderTier = NOT_SUPPORTED` is a *result* and passes; that is the
    /// whole point of running this. `crcbl-mtl`'s probe learnt this the
    /// expensive way, asserting twice on values a truthful device reported as
    /// zero and reddening the board both times.
    ///
    /// The one thing asserted is that the questions were **asked**: a refused
    /// [`check_feature`] is the *runtime* saying it has never heard of the
    /// query, not the device saying no, and those are opposite readings of this
    /// log. A run that printed nothing but refusals is a zero-checks pass in a
    /// new costume. Every refusal is collected rather than returned on, so one
    /// run names all of them — this suite costs a whole CI round trip per
    /// question, which is why `tests/run-dx12-e2e.sh` passes `--no-fail-fast`.
    ///
    /// The falsifying edit is swapping the two [`FeatureQuery`] ids above.
    /// D3D12 checks the payload size against the feature id and the two structs
    /// are different widths, so both queries are refused and the assertion fires
    /// naming them.
    ///
    /// nextest captures a passing test's stdout, so read this with
    /// `--success-output immediate`, which is what `tests/run-dx12-e2e.sh`
    /// passes.
    #[test]
    #[ignore = "needs a real D3D12 device; run tests/run-dx12-e2e.sh"]
    fn a_device_answers_whether_it_has_a_mesh_shader_tier_and_a_model_to_load_one() {
        let (instance, device) = open_device();
        let adapter = pinned_adapter(&instance);
        let record = instance
            .records()
            .iter()
            .find(|record| record.info.id == adapter)
            .expect("the pin resolved against this same enumeration");
        // Attribution first: every number below is about this adapter and no
        // other, and a reader of the log has no other way to know which. The
        // software flag is `is_software`'s answer rather than
        // `DXGI_ADAPTER_FLAG_SOFTWARE`, which is clear on the very rasteriser CI
        // pins.
        println!(
            "{MESH_PROBE}: adapter {:?} type={:?} software={} vendor={:#06x} device={:#06x} \
             driver={:?}",
            record.info.name,
            record.info.device_type,
            record.raw.software,
            record.info.vendor_id,
            record.info.device_id,
            record.info.driver,
        );

        let raw = device.raw();
        let mut refused: Vec<&str> = Vec::new();

        // The whole point of the test.
        let options7 = check_feature(raw, D3D12_FEATURE_DATA_D3D12_OPTIONS7::default());
        match options7 {
            Some(answer) => {
                println!(
                    "{MESH_PROBE}: MeshShaderTier = {} ({})",
                    answer.MeshShaderTier.0,
                    mesh_shader_tier_name(answer.MeshShaderTier)
                );
                println!(
                    "{MESH_PROBE}: SamplerFeedbackTier = {} ({})",
                    answer.SamplerFeedbackTier.0,
                    sampler_feedback_tier_name(answer.SamplerFeedbackTier)
                );
            }
            None => refused.push("D3D12_FEATURE_D3D12_OPTIONS7"),
        }

        // Asked at 6.6 rather than read: this query is **in/out**, and the
        // number that matters is not the device's ceiling but whether it reaches
        // the model the committed DXIL was built at. The runtime lowers the
        // value it is given, so an answer below what was asked is the device's
        // own limit.
        let negotiated = check_feature(
            raw,
            D3D12_FEATURE_DATA_SHADER_MODEL {
                HighestShaderModel: D3D_SHADER_MODEL_6_6,
            },
        );
        match negotiated {
            Some(answer) => println!(
                "{MESH_PROBE}: SHADER_MODEL asked {} -> {}",
                shader_model_name(D3D_SHADER_MODEL_6_6),
                shader_model_name(answer.HighestShaderModel),
            ),
            None => refused.push("D3D12_FEATURE_SHADER_MODEL at 6.6"),
        }
        // Beside it, `highest_shader_model`'s descending probe, which still
        // answers on a runtime too old to be asked about 6.6 at all — so the log
        // carries this device's real ceiling even on the run where the line
        // above is a refusal.
        println!(
            "{MESH_PROBE}: highest shader model by descending probe = {}",
            shader_model_name(highest_shader_model(raw))
        );

        match check_feature(raw, D3D12_FEATURE_DATA_D3D12_OPTIONS::default()) {
            Some(answer) => println!(
                "{MESH_PROBE}: ResourceBindingTier = {}",
                answer.ResourceBindingTier.0
            ),
            None => refused.push("D3D12_FEATURE_D3D12_OPTIONS"),
        }

        match check_feature(raw, D3D12_FEATURE_DATA_D3D12_OPTIONS3::default()) {
            Some(answer) => println!(
                "{MESH_PROBE}: CopyQueueTimestampQueriesSupported = {}",
                answer.CopyQueueTimestampQueriesSupported.as_bool()
            ),
            None => refused.push("D3D12_FEATURE_D3D12_OPTIONS3"),
        }

        // The conclusion, so the log carries it rather than only the inputs it
        // was drawn from. A refused query counts as the answer that keeps this
        // backend where it already is, which is the direction a capability must
        // fail in — and the assertion below is what says the reading was refused
        // rather than measured.
        let tier = options7.map_or(D3D12_MESH_SHADER_TIER_NOT_SUPPORTED, |answer| {
            answer.MeshShaderTier
        });
        let model = negotiated.map_or(D3D_SHADER_MODEL_NONE, |answer| answer.HighestShaderModel);
        let verdict = if tier.0 < D3D12_MESH_SHADER_TIER_1.0 {
            "no mesh tier here — MeshShading and TaskShaderStage can be implemented on this \
             backend and never proved on this device"
        } else if model.0 < D3D_SHADER_MODEL_6_6.0 {
            "a mesh tier with no shader model to load the committed DXIL — crcbl-shaders ships \
             dxil-model = 6_6, so this device could not run it whatever its hardware can do"
        } else {
            "MeshShading and TaskShaderStage are ordinary Unwritten work and are provable on \
             this device"
        };
        println!("{MESH_PROBE}: verdict — {verdict}");

        assert!(
            refused.is_empty(),
            "the measurement did not happen: D3D12 refused {refused:?} on {:?}. A refusal is this \
             Windows runtime saying it has never heard of the query, not the device answering no, \
             and the two are opposite readings of the log above. A tier reported as NOT_SUPPORTED \
             is a result and passes; this is a run that asked nothing.",
            record.info.name,
        );
    }
}
