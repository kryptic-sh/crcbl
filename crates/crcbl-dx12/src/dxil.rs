//! The DXIL container, read as bytes — the one part of this backend that holds
//! no `windows` type and can therefore be proven on the machine it is written
//! on.
//!
//! `crcbl_dx12::present` is here for the same reason and says so; this module is
//! the second, and it carries more weight, because everything it answers is a
//! fact about a *committed artifact* rather than about a driver. A container
//! that is not what this crate thinks it is fails at
//! `CreateGraphicsPipelineState` on a runner, four minutes and one CI round trip
//! away.
//!
//! # What is read, and what each answer is for
//!
//! * The **container header** — magic, digest, declared size, part table — so a
//!   truncated, unsigned or mis-sliced artifact is refused by name. See
//!   [`Dxil::parse`].
//! * The `DXIL` part's **shader kind**, so a pixel container in a vertex slot is
//!   "this is the wrong artifact" rather than a driver's opinion about a
//!   pipeline.
//! * The `PSV0` part's **thread-group size**, which is what makes
//!   [`ComputePipelineDesc::workgroup_size`](crcbl_hal::ComputePipelineDesc::workgroup_size)
//!   checkable here. `crcbl-vk` reads the same number out of SPIR-V's
//!   `OpExecutionMode LocalSize` and refuses a descriptor that disagrees; this
//!   is D3D12's equivalent, and the reason it is possible at all is that
//!   `[numthreads(x, y, z)]` is baked into the container. Metal is the backend
//!   that genuinely cannot, and the seam's field exists for it.
//!
//! # `PSV0` is versioned by size, which is what makes reading it safe
//!
//! The **Pipeline State Validation** part opens with a `u32` giving the byte
//! length of the `PSVRuntimeInfo` that follows, and every version of that struct
//! extends the previous one — the format's own rule, not a convention adopted
//! here. `PSVRuntimeInfo2` is the first to carry `NumThreadsX/Y/Z`, so a
//! container whose runtime info is shorter than [`PSV_RUNTIME_INFO_2`] simply
//! does not say, and [`Dxil::numthreads`] is `None`. Every artifact
//! `crcbl-shaders` commits does say — see this module's tests, which read the
//! real files.
//!
//! # The register a binding lands on is its binding number
//!
//! Every source in `crcbl-shaders` declares each resource's D3D12 register
//! beside its `[[vk::binding(binding, set)]]`, as
//! `register(<class><binding>, space<set>)`, so a set holding a
//! `ConstantBuffer`, a `StructuredBuffer` and an `RWStructuredBuffer` at
//! bindings 0, 1 and 2 of set 1 is `b0`, `t1` and `u2` in space 1 — and a
//! push-constant block is `b0` in `crate::root::PUSH_CONSTANT_SPACE`.
//! `crate::root::assign_registers` is the same rule on the root-signature side.
//!
//! This module's tests hold the resource table of every committed container to
//! the bindings its own SPIR-V declares, so the rule is measured rather than
//! asserted.

use crcbl_hal::{HalError, ShaderModuleDesc, ShaderSources, ShaderStages};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_BYTECODE;

/// `DXIL::ShaderKind` for a pixel shader. Fixed by the container format.
pub(crate) const KIND_PIXEL: u32 = 0;
/// `DXIL::ShaderKind` for a vertex shader.
pub(crate) const KIND_VERTEX: u32 = 1;
/// `DXIL::ShaderKind` for a compute shader.
pub(crate) const KIND_COMPUTE: u32 = 5;
/// `DXIL::ShaderKind` for a mesh shader.
pub(crate) const KIND_MESH: u32 = 13;
/// `DXIL::ShaderKind` for an amplification shader — what the seam and Vulkan
/// call the **task** stage, and what D3D12's `as_6_6` profile compiles.
///
/// Not adjacent to [`KIND_MESH`] by accident: the format numbers the two
/// together at the end of the enumeration, after the ray-tracing kinds, so
/// neither can be guessed from the graphics kinds above. Both are measured
/// against the committed artifacts by
/// `the_committed_mesh_shaders_carry_the_two_kinds_this_backend_expects`.
pub(crate) const KIND_AMPLIFICATION: u32 = 14;

/// Bytes of `PSVRuntimeInfo2`, the first version of the struct to carry a
/// thread-group size. Fixed by the container format.
const PSV_RUNTIME_INFO_2: usize = 48;

/// Byte offset of `NumThreadsX` within `PSVRuntimeInfo2`; `Y` and `Z` follow it.
/// Fixed by the container format.
const PSV_NUM_THREADS: usize = 36;

/// A validated DXIL container.
///
/// Holds the bytes so the module owns them: a `D3D12_SHADER_BYTECODE` is a
/// borrowed pointer, and the caller's slice does not outlive
/// `create_shader_module`.
#[derive(Debug)]
pub(crate) struct Dxil {
    /// The container itself. [`bytecode`](Self::bytecode) is its only reader and
    /// exists on Windows alone, so off Windows this is held and never looked at
    /// — the parse is the whole job there.
    #[cfg_attr(
        not(target_os = "windows"),
        expect(
            dead_code,
            reason = "read by `bytecode`, which needs a D3D12 type to return"
        )
    )]
    bytes: Vec<u8>,
    /// `DXIL::ShaderKind` from the container's `DXIL` part.
    kind: u32,
    /// `[numthreads(x, y, z)]`, when the `PSV0` part is new enough to say.
    numthreads: Option<[u32; 3]>,
    /// The structured buffers the `DXIL` part's bitcode declares, or why it
    /// could not be read.
    ///
    /// Kept as a `Result` rather than refused at parse, because only a pipeline
    /// needs the answer — see [`require_storage_strides`](Self::require_storage_strides),
    /// which is where a failure reaches the caller.
    structured: Result<Vec<crate::bitcode::StructuredBuffer>, String>,
}

impl Dxil {
    /// Parses and validates a container. See the module docs for what is
    /// refused and why each is refused *here*.
    ///
    /// The layout is the format's, fixed outside this repository:
    ///
    /// ```text
    /// 0  u32     magic "DXBC"
    /// 4  [u8;16] digest, all-zero until libdxil.so signs it
    /// 20 u16 u16 container version
    /// 24 u32     container size in bytes
    /// 28 u32     part count
    /// 32 u32[]   part offsets; each part is a fourcc, a u32 size, then data
    /// ```
    ///
    /// The `DXIL` part's data opens with a `DxilProgramHeader`, whose first word
    /// packs the shader kind and model as `kind << 16 | major << 4 | minor`.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] naming which of the container's
    /// invariants failed, with `label` so the message points at the module
    /// rather than at the backend.
    pub(crate) fn parse(bytes: &[u8], label: &str) -> Result<Self, HalError> {
        let refuse = |why: &str| {
            HalError::ShaderCompilation(format!(
                "the DXIL for shader module `{label}` {why}; it is {} bytes",
                bytes.len()
            ))
        };
        if bytes.get(..4) != Some(b"DXBC") {
            return Err(refuse("does not open with the DXBC container magic"));
        }
        // `get` rather than an index: four bytes of magic do not promise twenty
        // of header, and a slice index would panic on a file that is exactly the
        // magic — which is what a zero-length download truncated to its first
        // block looks like.
        let Some(digest) = bytes.get(4..20) else {
            return Err(refuse("is too short to hold a container digest"));
        };
        if digest == [0u8; 16] {
            return Err(refuse(
                "has an all-zero container digest, so dxc never signed it — every D3D12 driver \
                 refuses an unsigned container",
            ));
        }
        let declared =
            word(bytes, 24).ok_or_else(|| refuse("is too short to hold a container header"))?;
        if declared as usize != bytes.len() {
            return Err(refuse(&format!(
                "declares a container of {declared} bytes, so it is truncated or mis-sliced"
            )));
        }
        let mut kind = None;
        let mut numthreads = None;
        let mut structured = Err(String::from("the container has no DXIL part"));
        for part in parts(bytes).ok_or_else(|| refuse("has a truncated part table"))? {
            match part.fourcc {
                b"DXIL" => {
                    let version = word(bytes, part.data)
                        .ok_or_else(|| refuse("has a truncated DXIL part"))?;
                    kind = Some(version >> 16);
                    structured = part
                        .data
                        .checked_sub(4)
                        .and_then(|at| word(bytes, at))
                        .and_then(|size| {
                            bytes.get(part.data..part.data.checked_add(size as usize)?)
                        })
                        .ok_or_else(|| String::from("the DXIL part runs past the container"))
                        .and_then(crate::bitcode::structured_buffers);
                }
                b"PSV0" => numthreads = psv_numthreads(bytes, part.data),
                _ => {}
            }
        }
        let kind = kind.ok_or_else(|| refuse("has no DXIL part, so it carries no bytecode"))?;
        Ok(Self {
            bytes: bytes.to_vec(),
            kind,
            numthreads,
            structured,
        })
    }

    /// Checks every storage buffer a pipeline layout declares against the
    /// structured buffer this container declares at the same register, and
    /// refuses a stride that disagrees.
    ///
    /// This is `crcbl-vk`'s `require_storage_strides`, read from DXIL: the
    /// layout's [`BindingKind::StorageBuffer`](crcbl_hal::BindingKind::StorageBuffer)
    /// stride becomes the view's `StructureByteStride`, and a view whose stride
    /// is not the shader's element addresses the wrong bytes on hardware. A
    /// declared buffer this stage does not use is not compared — one layout
    /// serves every stage, and each container declares only what it reads.
    ///
    /// The register and space a binding is compared at are the ones
    /// `crate::binding` assigned it — its binding number and its set; a layout
    /// whose registers do not line up with the container fails here naming both,
    /// rather than at a draw.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] naming the entry point, the binding and
    /// both strides — or why the container's bitcode could not be read.
    pub(crate) fn require_storage_strides(
        &self,
        declared: &[StorageRegister],
        entry_point: &str,
    ) -> Result<(), HalError> {
        if declared.is_empty() {
            return Ok(());
        }
        let structured = self.structured.as_ref().map_err(|reason| {
            HalError::ShaderCompilation(format!(
                "the container for `{entry_point}` cannot be checked against its layout's storage \
                 buffers: {reason}"
            ))
        })?;
        for layout in declared {
            let class = match layout.class {
                RegisterClass::Srv => crate::bitcode::BufferClass::ShaderResource,
                RegisterClass::Uav => crate::bitcode::BufferClass::UnorderedAccess,
                RegisterClass::Cbv | RegisterClass::Sampler => continue,
            };
            let Some(shader) = structured.iter().find(|buffer| {
                buffer.class == class
                    && buffer.space == layout.set
                    && buffer.register == layout.register
            }) else {
                continue;
            };
            if shader.stride != layout.stride {
                return Err(HalError::ShaderCompilation(format!(
                    "`{entry_point}`: the storage buffer at set {} binding {} is declared with a \
                     stride of {} bytes, and the container's structured buffer at {:?} register {} \
                     has {}-byte elements. BindingKind::StorageBuffer's stride is the size of one \
                     element of the array the shader declares",
                    layout.set,
                    layout.binding,
                    layout.stride,
                    layout.class,
                    layout.register,
                    shader.stride
                )));
            }
        }
        Ok(())
    }

    /// `[numthreads(x, y, z)]` as the container declares it, or `None` when the
    /// container is too old to carry it. See the module docs.
    pub(crate) const fn numthreads(&self) -> Option<[u32; 3]> {
        self.numthreads
    }

    /// The bytecode, as D3D12 takes it.
    ///
    /// Borrowed from `self`, so the returned value must not outlive the module —
    /// which is why every caller builds it inside the same statement that hands
    /// the pipeline descriptor to D3D12.
    #[cfg(target_os = "windows")]
    pub(crate) fn bytecode(&self) -> D3D12_SHADER_BYTECODE {
        D3D12_SHADER_BYTECODE {
            pShaderBytecode: self.bytes.as_ptr().cast(),
            BytecodeLength: self.bytes.len(),
        }
    }

    /// Checks the container is for `stage`, or says which it is for.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] naming both stages, or naming `stage`
    /// itself when it is not one this backend has a container kind for — see
    /// [`expected`].
    pub(crate) fn expect(&self, stage: ShaderStages, entry_point: &str) -> Result<(), HalError> {
        let Some((wanted, name)) = expected(stage) else {
            return Err(HalError::ShaderCompilation(format!(
                "the module bound as `{entry_point}` names {stage:?}, which is not one shader \
                 stage; a DXIL container is compiled for one entry point at one stage, so there \
                 is no single kind to check this one against"
            )));
        };
        if self.kind == wanted {
            return Ok(());
        }
        Err(HalError::ShaderCompilation(format!(
            "the module bound as `{entry_point}` for the {name} stage holds a {} container; a \
             DXIL container is compiled for one entry point at one stage, so this is the wrong \
             artifact rather than the wrong entry-point name",
            stage_name_of_kind(self.kind),
        )))
    }
}

/// A shader module: one validated DXIL container per entry point the caller
/// offered one for.
///
/// It lives in this module rather than beside the pipeline state objects it
/// feeds because it holds no `windows` type — so the lookup below, and the
/// refusal it owes a caller, are provable on a machine with no D3D12.
#[derive(Debug)]
pub(crate) struct ShaderModuleEntry {
    /// The device that created it. Read through
    /// [`Owned`](crate::handle::Owned), whose implementation is beside the rest
    /// of that trait's and therefore exists on Windows alone.
    #[cfg_attr(
        not(target_os = "windows"),
        expect(
            dead_code,
            reason = "read through `handle::Owned`, which is Windows-only"
        )
    )]
    pub(crate) owner: u64,
    /// The label the module was created with, so [`container`] names the
    /// artifact rather than only the entry point.
    ///
    /// [`container`]: ShaderModuleEntry::container
    label: String,
    /// One container per entry point, in the order the caller offered them.
    ///
    /// Pairs rather than a map, for the reason
    /// [`ShaderModuleDesc::dxil`](crcbl_hal::ShaderModuleDesc::dxil) gives: a
    /// module has a handful of entry points and the scan is nothing beside
    /// creating a pipeline state object.
    containers: Vec<(String, Dxil)>,
}

impl ShaderModuleEntry {
    /// The container compiled for `entry_point`.
    ///
    /// **This is where a two-stage module stops being a D3D12 problem.** The
    /// other three backends put every entry point into one artifact and find
    /// the one they want by name inside it; here the name selects among
    /// containers, which is the same lookup one level up.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] naming the entry point asked for and the
    /// ones this module holds — the failure a call site that offered a
    /// container for only one of its two stages produces, and one no other
    /// backend can see, because the other three artifacts carry every entry
    /// point at once and never make the pairing.
    pub(crate) fn container(&self, entry_point: &str) -> Result<&Dxil, HalError> {
        self.containers
            .iter()
            .find(|(name, _)| name == entry_point)
            .map(|(_, dxil)| dxil)
            .ok_or_else(|| {
                HalError::ShaderCompilation(format!(
                    "shader module `{}` holds DXIL for {}, and this stage names `{entry_point}`; a \
                     container is compiled for one entry point, so a module used at two stages has \
                     to be given a container for each",
                    self.label,
                    self.containers
                        .iter()
                        .map(|(name, _)| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
            })
    }
}

/// Validates every container a caller offered and wraps them in a module entry.
///
/// # Errors
///
/// [`HalError::ShaderCompilation`] when the descriptor carries no DXIL — named
/// through [`ShaderModuleDesc::unusable`](crcbl_hal::ShaderModuleDesc::unusable),
/// so the message says which formats were offered — or when any container fails
/// [`Dxil::parse`].
pub(crate) fn module(
    desc: &ShaderModuleDesc<'_>,
    owner: u64,
) -> Result<ShaderModuleEntry, HalError> {
    // DXIL and nothing else. `desc.spirv`, `.wgsl` and `.msl` are ignored rather
    // than translated: a cross-compiler inside this backend would be a second
    // shader compiler solving a problem `crcbl-shaders` already solved offline,
    // with a pinned compiler and a hashed, signed artifact.
    if desc.dxil.is_empty() {
        return Err(desc.unusable(ShaderSources::DXIL));
    }
    let label = desc.label.unwrap_or("<unlabelled>");
    let mut containers = Vec::with_capacity(desc.dxil.len());
    for (entry_point, bytes) in desc.dxil {
        // Every container up front rather than at the pipeline that reaches for
        // one: a module offering three stages and a truncated container for the
        // third is a broken artifact, and finding that out at
        // `create_shader_module` puts the error where the bytes came from.
        //
        // `label:entry` because a module now holds several containers, and the
        // label alone would not say which of them the parser objected to.
        containers.push((
            (*entry_point).to_owned(),
            Dxil::parse(bytes, &format!("{label}:{entry_point}"))?,
        ));
    }
    Ok(ShaderModuleEntry {
        owner,
        label: label.to_owned(),
        containers,
    })
}

/// One part of a container: its four-character code and where its data starts.
struct Part<'a> {
    fourcc: &'a [u8],
    data: usize,
}

/// Every part the container's table names, or `None` if the table is truncated.
fn parts(bytes: &[u8]) -> Option<Vec<Part<'_>>> {
    let count = word(bytes, 28)? as usize;
    let mut found = Vec::with_capacity(count);
    for part in 0..count {
        let offset = word(bytes, 32 + part * 4)? as usize;
        // A part is a fourcc, a `u32` size, then the data — so the data starts
        // eight bytes in, and a part whose header runs off the end is a
        // truncated table rather than a part with no name.
        found.push(Part {
            fourcc: bytes.get(offset..offset + 4)?,
            data: offset.checked_add(8)?,
        });
    }
    Some(found)
}

/// One little-endian word at `at`, or `None` past the end.
fn word(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
}

/// The thread-group size in a `PSV0` part starting at `data`, if it carries one.
///
/// `None` covers both "this container's runtime info predates the field" and "a
/// stage that has no thread group", which read the same here and are told apart
/// by the caller: a graphics container's `NumThreads` is zero, and
/// [`Dxil::expect`] has already refused a graphics container in a compute slot
/// before anything asks.
fn psv_numthreads(bytes: &[u8], data: usize) -> Option<[u32; 3]> {
    let info_size = word(bytes, data)? as usize;
    if info_size < PSV_RUNTIME_INFO_2 {
        return None;
    }
    let info = data.checked_add(4)?.checked_add(PSV_NUM_THREADS)?;
    let size = [
        word(bytes, info)?,
        word(bytes, info + 4)?,
        word(bytes, info + 8)?,
    ];
    if size.contains(&0) {
        return None;
    }
    Some(size)
}

/// The container kind one stage must hold, and how a message names that stage.
///
/// **One table rather than two**, so a stage cannot be named in a message it was
/// not checked against.
///
/// `None` for a set that is not exactly one stage — which is what a stage bit
/// added to [`ShaderStages`] after this was written would arrive as. That case
/// is the reason this is a `match` with no catch-all mapping onto a kind: the
/// arm it used to fall into was [`KIND_COMPUTE`], so a mesh container checked
/// against a mesh stage read as "this is not a compute shader" — a refusal with
/// the wrong sentence — and, worse, a *compute* container in a mesh slot passed.
const fn expected(stages: ShaderStages) -> Option<(u32, &'static str)> {
    match stages {
        ShaderStages::VERTEX => Some((KIND_VERTEX, "vertex")),
        ShaderStages::FRAGMENT => Some((KIND_PIXEL, "fragment")),
        ShaderStages::COMPUTE => Some((KIND_COMPUTE, "compute")),
        ShaderStages::MESH => Some((KIND_MESH, "mesh")),
        ShaderStages::TASK => Some((KIND_AMPLIFICATION, "task")),
        _ => None,
    }
}

/// How a message names a container's own shader kind.
///
/// D3D12's word for the seam's task stage is **amplification**, and this is a
/// message about an artifact `dxc` produced, so it uses D3D12's.
const fn stage_name_of_kind(kind: u32) -> &'static str {
    match kind {
        KIND_PIXEL => "pixel",
        KIND_VERTEX => "vertex",
        KIND_COMPUTE => "compute",
        KIND_MESH => "mesh",
        KIND_AMPLIFICATION => "amplification",
        _ => "non-graphics",
    }
}

/// Which HLSL register file a binding lands in.
///
/// Four independent numbering spaces, which is the whole reason a binding
/// number is not a register: `b`, `t`, `u` and `s` each start at zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RegisterClass {
    /// `bN` — a constant buffer.
    Cbv,
    /// `tN` — a read-only resource.
    Srv,
    /// `uN` — a read-write resource.
    Uav,
    /// `sN` — a sampler.
    Sampler,
}

/// One storage buffer a pipeline layout declares, at the register
/// `crate::binding` assigned it — what [`Dxil::require_storage_strides`] holds
/// each container to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StorageRegister {
    /// The set, which is also the register space.
    pub(crate) set: u32,
    /// The seam's binding number, for the message.
    pub(crate) binding: u32,
    /// `t` for a read-only storage buffer, `u` for a writable one.
    pub(crate) class: RegisterClass,
    pub(crate) register: u32,
    /// [`BindingKind::StorageBuffer`](crcbl_hal::BindingKind::StorageBuffer)'s
    /// stride.
    pub(crate) stride: u32,
}

/// One record of a container's `PSV0` resource table.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PsvResource {
    /// The register file it is declared in.
    pub(crate) class: RegisterClass,
    /// The register space.
    pub(crate) space: u32,
    /// The first register it covers.
    pub(crate) lower: u32,
    /// The last register it covers — above `lower` for an array of
    /// descriptors, and `u32::MAX` for an unbounded one.
    pub(crate) upper: u32,
    /// `PSVResourceKind`: what the shader declared it as — a `Texture2D`, a
    /// `StructuredBuffer`, a `cbuffer`. `None` for a record too old to carry
    /// it.
    pub(crate) kind: Option<u32>,
}

/// A container's `PSV0` resource table: every resource the entry point reads,
/// where, and as what.
///
/// Test-only, because the runtime path never needs it: the root signature is
/// built from the caller's declared layout, and this is how that layout is
/// checked against the artifact rather than a second source for it.
///
/// The table follows the runtime info: a `u32` count, a `u32` stride, then
/// that many records. Each opens `ResType`, `Space`, `LowerBound`,
/// `UpperBound` — `PSVResourceBindInfo0` — and a record of 24 bytes or more
/// is a `PSVResourceBindInfo1`, which goes on with `ResKind` and `ResFlags`.
/// `PSVResourceType` numbers sampler 1, CBV 2, the three SRV kinds 3–5 and
/// the UAV kinds from 6. All of it is fixed by the container format.
///
/// # Panics
///
/// On a container without a well-formed `PSV0` part, which no committed
/// artifact is.
#[cfg(test)]
pub(crate) fn psv_resources(bytes: &[u8]) -> Vec<PsvResource> {
    /// Bytes of a `PSVResourceBindInfo1`, the first record to carry a kind.
    const BIND_INFO_1: usize = 24;
    let part = parts(bytes)
        .expect("a well-formed part table")
        .into_iter()
        .find(|part| part.fourcc == b"PSV0")
        .expect("a PSV0 part");
    let info_size = word(bytes, part.data).expect("a runtime info size") as usize;
    let mut at = part.data + 4 + info_size;
    let count = word(bytes, at).expect("a resource count") as usize;
    at += 4;
    if count == 0 {
        return Vec::new();
    }
    let stride = word(bytes, at).expect("a resource record stride") as usize;
    at += 4;
    (0..count)
        .map(|index| {
            let record = at + index * stride;
            let class = match word(bytes, record).expect("a resource type") {
                1 => RegisterClass::Sampler,
                2 => RegisterClass::Cbv,
                3..=5 => RegisterClass::Srv,
                other => {
                    assert!(
                        other >= 6,
                        "PSVResourceType {other} is not a bound resource"
                    );
                    RegisterClass::Uav
                }
            };
            PsvResource {
                class,
                space: word(bytes, record + 4).expect("a register space"),
                lower: word(bytes, record + 8).expect("a lower bound"),
                upper: word(bytes, record + 12).expect("an upper bound"),
                kind: (stride >= BIND_INFO_1)
                    .then(|| word(bytes, record + 16).expect("a resource kind")),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal signed container, built to the format's own layout: magic, a
    /// non-zero digest, a version, the size, one part offset, and a `DXIL` part
    /// whose program header names `kind`.
    fn container(kind: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"DXBC");
        bytes.extend_from_slice(&[0xAB; 16]);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        // Size and part count are patched below, once the length is known.
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&36u32.to_le_bytes());
        bytes.extend_from_slice(b"DXIL");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&((kind << 16) | (6 << 4) | 6).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let size = u32::try_from(bytes.len()).expect("a small container");
        bytes[24..28].copy_from_slice(&size.to_le_bytes());
        bytes
    }

    /// A well-formed container parses, and its shader kind comes out.
    ///
    /// The positive case is asserted first so every refusal below is the check
    /// firing rather than the parser refusing everything.
    #[test]
    fn a_signed_container_parses_and_reports_its_stage() {
        let vertex = Dxil::parse(&container(KIND_VERTEX), "triangle").expect("a vertex container");
        assert_eq!(vertex.kind, KIND_VERTEX);
        vertex
            .expect(ShaderStages::VERTEX, "vertexMain")
            .expect("a vertex container in the vertex slot");

        let error = vertex
            .expect(ShaderStages::FRAGMENT, "fragmentMain")
            .expect_err("a vertex container is not a pixel shader");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("fragmentMain"), "{text}");
        assert!(text.contains("vertex container"), "{text}");

        // A container with no `PSV0` part says nothing about a thread group,
        // which is the arm that must not become "one thread".
        assert_eq!(vertex.numthreads(), None);
    }

    /// A descriptor with no DXIL says which formats it had, rather than which
    /// slice is missing — the seam's own message, so it reads the same on every
    /// backend.
    #[test]
    fn a_module_without_dxil_names_the_formats_it_was_given() {
        let error = module(
            &ShaderModuleDesc {
                label: Some("mesh.slang"),
                spirv: &[0x0723_0203, 0, 0, 0, 0],
                wgsl: Some("@vertex fn vertexMain() {}"),
                ..ShaderModuleDesc::default()
            },
            1,
        )
        .expect_err("this backend compiles DXIL and nothing else");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("mesh.slang"), "{text}");
        assert!(text.contains("SPIR-V and WGSL"), "{text}");
        assert!(text.contains("only compile DXIL"), "{text}");
    }

    /// **One module, two stages: each entry point resolves to its own
    /// container, and a stage the module was not given one for is refused by
    /// name.**
    ///
    /// This is what lets `crcbl-render`'s graphics passes create a single
    /// shader module the way every other backend does. The two halves are both
    /// necessary: without the lookup a pipeline would be built from whichever
    /// container came first — a pixel shader in the vertex slot, which only
    /// `CreateGraphicsPipelineState` on a Windows runner would ever notice —
    /// and without the refusal a call site that offered one container for a
    /// two-stage module would be silently half-built.
    #[test]
    fn each_stage_of_a_two_stage_module_resolves_to_its_own_container() {
        let vertex = container(KIND_VERTEX);
        let fragment = container(KIND_PIXEL);
        let entry = module(
            &ShaderModuleDesc {
                label: Some("mesh.slang"),
                dxil: &[("vertexMain", &vertex), ("fragmentMain", &fragment)],
                ..ShaderModuleDesc::default()
            },
            1,
        )
        .expect("two signed containers are one module");

        // Each name selects its own container, and `expect` agrees about the
        // stage — so a swapped pair would fail here rather than at a driver.
        entry
            .container("vertexMain")
            .expect("the vertex container")
            .expect(ShaderStages::VERTEX, "vertexMain")
            .expect("a vertex container in the vertex slot");
        entry
            .container("fragmentMain")
            .expect("the fragment container")
            .expect(ShaderStages::FRAGMENT, "fragmentMain")
            .expect("a pixel container in the fragment slot");

        let error = entry
            .container("computeMain")
            .expect_err("this module was given no compute container");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("mesh.slang"), "{text}");
        assert!(text.contains("computeMain"), "{text}");
        assert!(text.contains("`vertexMain`, `fragmentMain`"), "{text}");
    }

    /// **The committed artifacts go through the same lookup**, so the pairing
    /// is checked against the real containers rather than only against ones
    /// this test built.
    ///
    /// `crcbl-shaders` hands over the pairs and this resolves each back, which
    /// is the join between the two crates: a `dxil_containers` that mislabelled
    /// a container would put a pixel shader in the vertex slot, and the stage
    /// check is what refuses it.
    ///
    /// **Every entry point the module commits, not one per stage.**
    /// `mesh.slang` has two vertex entry points — the colour pass's and the
    /// depth-only one `crcbl_render::forward`'s shadow atlas and depth prepass
    /// run — so a lookup by stage answers `None` for it, and a container the
    /// list did not reach would be one nothing here ever parsed.
    #[test]
    fn the_committed_graphics_shaders_resolve_every_entry_point() {
        for shader in [&crcbl_shaders::MESH, &crcbl_shaders::UI] {
            let entry = module(
                &ShaderModuleDesc {
                    label: Some(shader.name()),
                    dxil: &shader.dxil_containers(),
                    ..ShaderModuleDesc::default()
                },
                1,
            )
            .unwrap_or_else(|error| panic!("{}: {error}", shader.name()));
            assert!(
                !shader.entry_points().is_empty(),
                "{}: nothing to resolve",
                shader.name()
            );
            for entry_point in shader.entry_points() {
                let stages = match entry_point.stage() {
                    crcbl_shaders::Stage::Vertex => ShaderStages::VERTEX,
                    crcbl_shaders::Stage::Fragment => ShaderStages::FRAGMENT,
                    // Neither of these two has one. A stage arriving here is a
                    // module that grew a pipeline shape this test does not
                    // describe, which is a finding rather than a container to
                    // wave through.
                    stage => panic!(
                        "{}: {} is a {stage:?} stage, which is not one of the graphics pair \
                         this test resolves",
                        shader.name(),
                        entry_point.name()
                    ),
                };
                entry
                    .container(entry_point.name())
                    .unwrap_or_else(|error| panic!("{}: {error}", shader.name()))
                    .expect(stages, entry_point.name())
                    .unwrap_or_else(|error| panic!("{}: {error}", shader.name()));
            }
        }
    }

    /// **[`KIND_MESH`] and [`KIND_AMPLIFICATION`] are the numbers the committed
    /// artifacts actually carry**, not the numbers `DXIL::ShaderKind` was
    /// remembered as.
    ///
    /// This is the measurement the two constants rest on, and it is worth its
    /// own test because they are the only kinds in this module with no other
    /// check behind them: a wrong value would let a *task* container into the
    /// mesh slot, which `CreatePipelineState` reports as a pipeline it does not
    /// like rather than as the wrong artifact — and would do it on a Windows
    /// runner, one CI round trip from here.
    ///
    /// Both shaders are listed because `mesh_shader.slang` and
    /// `mesh_cluster.slang` are compiled separately, and `amplifiedMeshMain` is
    /// listed beside `meshMain` because the two are different entry points of
    /// one stage — a container table that paired an entry point with the wrong
    /// stage would show here.
    #[test]
    fn the_committed_mesh_shaders_carry_the_two_kinds_this_backend_expects() {
        let cases: &[(&str, &crcbl_shaders::Shader)] = &[
            ("mesh_shader", &crcbl_shaders::MESH_SHADER),
            ("mesh_cluster", &crcbl_shaders::MESH_CLUSTER),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (name, shader) in cases {
            let entry = module(
                &ShaderModuleDesc {
                    label: Some(name),
                    dxil: &shader.dxil_containers(),
                    ..ShaderModuleDesc::default()
                },
                1,
            )
            .unwrap_or_else(|error| panic!("{name}: {error}"));
            for (entry_point, stage) in [
                ("taskMain", ShaderStages::TASK),
                ("meshMain", ShaderStages::MESH),
                ("amplifiedMeshMain", ShaderStages::MESH),
            ] {
                let dxil = entry
                    .container(entry_point)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                dxil.expect(stage, entry_point)
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                // And the other of the two kinds is refused, so the check above
                // is the kind matching rather than `expect` accepting anything
                // a mesh pipeline hands it.
                let other = if stage == ShaderStages::MESH {
                    ShaderStages::TASK
                } else {
                    ShaderStages::MESH
                };
                let error = dxil
                    .expect(other, entry_point)
                    .expect_err("the two mesh-pipeline stages are not interchangeable");
                let HalError::ShaderCompilation(text) = &error else {
                    panic!("{name}: {error:?}");
                };
                assert!(text.contains(entry_point), "{name}: {text}");
            }
        }
    }

    /// A stage set that is not exactly one stage is refused rather than
    /// checked against whichever kind a catch-all arm would have picked.
    ///
    /// The falsifying edit is restoring that arm: with `_ => KIND_COMPUTE` a
    /// compute container passes `expect(ShaderStages::MESH, …)`, which is
    /// exactly the artifact mix-up this whole module exists to catch.
    #[test]
    fn a_stage_set_that_is_not_one_stage_has_no_container_kind() {
        let compute = Dxil::parse(&container(KIND_COMPUTE), "probe").expect("a compute container");
        for stages in [
            ShaderStages::GRAPHICS,
            ShaderStages::ALL,
            ShaderStages::MESH | ShaderStages::TASK,
            ShaderStages::empty(),
        ] {
            let error = compute
                .expect(stages, "computeMain")
                .expect_err("a stage set with no single container kind");
            let HalError::ShaderCompilation(text) = &error else {
                panic!("{stages:?}: {error:?}");
            };
            assert!(text.contains("not one shader stage"), "{stages:?}: {text}");
        }
        // A mesh container is not a compute one, which is what the deleted
        // catch-all used to claim it was.
        let mesh = Dxil::parse(&container(KIND_MESH), "mesh_shader").expect("a mesh container");
        let error = mesh
            .expect(ShaderStages::COMPUTE, "meshMain")
            .expect_err("a mesh container in a compute slot");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("mesh container"), "{text}");
        assert!(text.contains("compute stage"), "{text}");
    }

    /// **Every way a container can be wrong is refused, by name.**
    ///
    /// The unsigned case is the one this exists for: an all-zero digest is what
    /// a `dxc` that could not load `libdxil.so` produces, it hashes and commits
    /// like any other artifact, and the only other thing that would ever notice
    /// is a driver at pipeline creation.
    #[test]
    fn a_container_that_is_not_signed_dxil_is_refused_by_name() {
        let mut unsigned = container(KIND_VERTEX);
        unsigned[4..20].fill(0);

        let mut truncated = container(KIND_VERTEX);
        truncated.truncate(truncated.len() - 4);

        let mut no_dxil_part = container(KIND_VERTEX);
        no_dxil_part[36..40].copy_from_slice(b"STAT");

        let cases: &[(&str, &[u8])] = &[
            ("DXBC container magic", b"not a container at all"),
            ("all-zero container digest", &unsigned),
            ("truncated or mis-sliced", &truncated),
            ("no DXIL part", &no_dxil_part),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (expected, bytes) in cases {
            let error = Dxil::parse(bytes, "triangle").expect_err(expected);
            let HalError::ShaderCompilation(text) = &error else {
                panic!("{expected}: a bad artifact is not {error:?}");
            };
            assert!(text.contains(expected), "{expected}: {text}");
            assert!(text.contains("triangle"), "the label must survive: {text}");
        }
    }

    /// **Every committed compute container declares the workgroup size
    /// `crcbl-shaders` publishes beside it**, and every graphics one declares
    /// none.
    ///
    /// This is the check that turns [`psv_numthreads`] from a struct layout
    /// recalled from a header into a measurement: the offsets are wrong if any
    /// of these disagrees, and `crcbl-shaders`' own tests already pin each
    /// `WORKGROUP_SIZE` against the `[numthreads(…)]` in its `.slang` source. So
    /// the two ends are independent and this joins them.
    ///
    /// It runs on every host, which is the point of this module living outside
    /// the `windows` cfg.
    #[test]
    fn the_committed_containers_carry_the_workgroup_size_crcbl_shaders_publishes() {
        let compute: &[(&str, &crcbl_shaders::Shader, &str, u32)] = &[
            (
                "compute_probe",
                &crcbl_shaders::COMPUTE_PROBE,
                "computeMain",
                crcbl_shaders::compute_probe::WORKGROUP_SIZE,
            ),
            (
                "cull",
                &crcbl_shaders::CULL,
                "computeMain",
                crcbl_shaders::cull::WORKGROUP_SIZE,
            ),
            // The two occlusion phases beside it, on its workgroup.
            (
                "cull",
                &crcbl_shaders::CULL,
                "occlusionMain",
                crcbl_shaders::cull::WORKGROUP_SIZE,
            ),
            (
                "cull",
                &crcbl_shaders::CULL,
                "lateMain",
                crcbl_shaders::cull::WORKGROUP_SIZE,
            ),
            (
                "draw_gen",
                &crcbl_shaders::DRAW_GEN,
                "binMain",
                crcbl_shaders::draw_gen::WORKGROUP_SIZE,
            ),
            // The prefix sum is one invocation, and `crcbl-shaders` pins its
            // `[numthreads(1, 1, 1)]` beside `WORKGROUP_SIZE` — so the literal
            // here is the shader's number, not a second one.
            ("draw_gen", &crcbl_shaders::DRAW_GEN, "startsMain", 1),
            (
                "draw_gen",
                &crcbl_shaders::DRAW_GEN,
                "scatterMain",
                crcbl_shaders::draw_gen::WORKGROUP_SIZE,
            ),
            (
                "draw_gen",
                &crcbl_shaders::DRAW_GEN,
                "lateScatterMain",
                crcbl_shaders::draw_gen::WORKGROUP_SIZE,
            ),
            (
                "draw_gen",
                &crcbl_shaders::DRAW_GEN,
                "lateFinishMain",
                crcbl_shaders::draw_gen::WORKGROUP_SIZE,
            ),
        ];
        assert!(!compute.is_empty(), "nothing to check");
        for (name, shader, entry, size) in compute {
            let bytes = shader
                .dxil(entry)
                .unwrap_or_else(|| panic!("{name} commits a DXIL container for {entry}"));
            let parsed = Dxil::parse(bytes, name).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                parsed.numthreads(),
                Some([*size, 1, 1]),
                "{name}'s {entry} container disagrees with its workgroup size"
            );
        }

        // And a graphics container has no thread group, so the field cannot be
        // read out of one by accident — which is what would happen if the offset
        // landed inside a signature element table instead.
        let vertex = crcbl_shaders::TRIANGLE
            .dxil("vertexMain")
            .expect("triangle commits a DXIL container");
        let parsed = Dxil::parse(vertex, "triangle").expect("a signed container");
        assert_eq!(parsed.numthreads(), None);
    }

    /// **Every committed container declares each resource at the register and
    /// space its own SPIR-V's binding and set name**, over every shader that
    /// commits one — the rule `crate::root::assign_registers` builds every root
    /// signature on, measured against the artifacts rather than restated.
    ///
    /// The SPIR-V is the other side because it is compiled from the same
    /// source, and `[[vk::binding(binding, set)]]` is what reaches it: a
    /// container whose resource is not at `(set, binding)` of some decorated
    /// SPIR-V resource is a container whose register a root signature built
    /// from the layout would miss. The one resource with no binding is a
    /// push-constant block, which must be `b0` in its own space.
    ///
    /// Each container declares only what its entry point reads, so the check
    /// is that every record is **among** the SPIR-V's slots — and, so that it
    /// cannot pass vacuously, that the records it checked are not empty.
    #[test]
    fn every_container_declares_each_resource_at_its_binding_and_set() {
        let compiled: Vec<&crcbl_shaders::Shader> = crcbl_shaders::ALL
            .iter()
            .copied()
            .filter(|shader| !shader.dxil_containers().is_empty())
            .collect();
        assert!(!compiled.is_empty(), "no shader commits a DXIL container");

        let mut checked = 0;
        for shader in compiled {
            let slots = crcbl_shaders::descriptor_slots(shader.spirv());
            for (entry_point, bytes) in shader.dxil_containers() {
                for record in psv_resources(bytes) {
                    checked += 1;
                    let pushed = record.class == RegisterClass::Cbv
                        && record.space == crate::root::PUSH_CONSTANT_SPACE
                        && record.lower == 0;
                    assert!(
                        pushed || slots.contains(&(record.space, record.lower)),
                        "{}:{entry_point}: the container declares {:?} register {} in space {}, \
                         and the SPIR-V decorates nothing at set {} binding {}; its (set, \
                         binding)s are {slots:?}",
                        shader.name(),
                        record.class,
                        record.lower,
                        record.space,
                        record.space,
                        record.lower,
                    );
                }
            }
        }
        assert!(checked > 0, "no container declared a resource at all");
    }

    /// **The walk reads what the containers say, not what the rule wants.**
    ///
    /// The test above passes for any container whose registers are a subset of
    /// its bindings, so it is held to two answers read by hand: `sprite`'s
    /// sheet is set 1 binding 0, which is `t0` in space 1 — a count run across
    /// the sets in space 0 would have made it `t1` in space 0 — and
    /// `mesh_cluster`'s task stage reads `clusters` at binding 32, which the
    /// counting rule put at `t6`, the register `mesh.slang`'s fragment stage
    /// gave the shadow atlas.
    #[test]
    fn the_resource_table_names_the_registers_the_bindings_do() {
        let fragment = crcbl_shaders::SPRITE
            .dxil("fragmentMain")
            .expect("sprite commits a fragment container");
        assert!(
            psv_resources(fragment)
                .iter()
                .any(|record| record.class == RegisterClass::Srv
                    && record.space == 1
                    && record.lower == 0),
            "sprite's sheet is not t0 in space 1: {:?}",
            psv_resources(fragment)
        );

        let task = crcbl_shaders::MESH_CLUSTER
            .dxil("taskMain")
            .expect("mesh_cluster commits a task container");
        let records = psv_resources(task);
        assert!(
            records
                .iter()
                .any(|record| record.class == RegisterClass::Srv
                    && record.space == 0
                    && record.lower == 32),
            "mesh_cluster's task stage does not read clusters at t32: {records:?}"
        );
    }

    /// **A declared storage stride is held to the container at its register.**
    /// The triangle's vertex container reads `StructuredBuffer<Vertex>` at `t0`,
    /// 32 bytes an element: the matching figure passes, a wrong one is refused
    /// naming the binding and both numbers, and a register the container does
    /// not declare is not compared — one layout serves every stage.
    ///
    /// On any host, because the check is a fact about a committed artifact.
    #[test]
    fn a_declared_stride_is_held_to_the_container_at_its_register() {
        let bytes = crcbl_shaders::TRIANGLE
            .dxil_containers()
            .into_iter()
            .find(|(entry, _)| *entry == "vertexMain")
            .map(|(_, bytes)| bytes)
            .expect("triangle commits a vertex container");
        let vertex = Dxil::parse(bytes, "triangle").expect("a committed container");
        let at = |register, stride| StorageRegister {
            set: 0,
            binding: 0,
            class: RegisterClass::Srv,
            register,
            stride,
        };

        vertex
            .require_storage_strides(&[at(0, 32)], "vertexMain")
            .expect("32 is the element's size");
        vertex
            .require_storage_strides(&[at(9, 4)], "vertexMain")
            .expect("a register this container does not declare is not compared");

        let error = vertex
            .require_storage_strides(&[at(0, 4)], "vertexMain")
            .expect_err("4 is not 32");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("set 0 binding 0"), "{text}");
        assert!(text.contains("stride of 4 bytes"), "{text}");
        assert!(text.contains("32-byte"), "{text}");
    }

    /// A container whose bitcode cannot be read says so when a pipeline needs
    /// the strides, rather than passing a check it could not make.
    #[test]
    fn a_container_without_readable_bitcode_refuses_a_stride_check() {
        let fake = Dxil::parse(&container(KIND_VERTEX), "fake").expect("a header-only container");
        let declared = [StorageRegister {
            set: 0,
            binding: 0,
            class: RegisterClass::Srv,
            register: 0,
            stride: 4,
        }];
        let error = fake
            .require_storage_strides(&declared, "vertexMain")
            .expect_err("nothing to check against");
        assert!(matches!(error, HalError::ShaderCompilation(_)), "{error:?}");
        fake.require_storage_strides(&[], "vertexMain")
            .expect("a layout with no storage buffers needs no bitcode");
    }
}
