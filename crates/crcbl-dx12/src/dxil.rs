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
//! # The register a binding lands on is *not* its binding number
//!
//! `[[vk::binding(binding, set)]]` is a Vulkan attribute. Slang's HLSL output
//! ignores it and lets `dxc` assign registers **per class, in declaration order,
//! across the whole source file, all in space 0** — so a set holding a
//! `ConstantBuffer`, a `StructuredBuffer` and an `RWStructuredBuffer` at
//! bindings 0, 1 and 2 is `b0`, `t0` and `u0` in the artifact, not `b0`, `t1`
//! and `u2`.
//!
//! [`Registers`] is that rule, and `crcbl_dx12::binding` runs it over a pipeline
//! layout's sets in order. It is correct only because `crcbl-shaders`'
//! `declaration_order` lint already requires every source to declare its
//! resources in ascending `(set, binding)` order — the same guarantee
//! `crcbl-mtl` depends on for its flat argument tables, and for the same reason.
//!
//! This module's tests check the rule against the resource table in every
//! committed container, so it is measured rather than asserted.

use crcbl_hal::{HalError, ShaderModuleDesc, ShaderSources, ShaderStages};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Direct3D12::D3D12_SHADER_BYTECODE;

/// `DXIL::ShaderKind` for a pixel shader. Fixed by the container format.
pub(crate) const KIND_PIXEL: u32 = 0;
/// `DXIL::ShaderKind` for a vertex shader.
pub(crate) const KIND_VERTEX: u32 = 1;
/// `DXIL::ShaderKind` for a compute shader.
pub(crate) const KIND_COMPUTE: u32 = 5;

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
        for part in parts(bytes).ok_or_else(|| refuse("has a truncated part table"))? {
            match part.fourcc {
                b"DXIL" => {
                    let version = word(bytes, part.data)
                        .ok_or_else(|| refuse("has a truncated DXIL part"))?;
                    kind = Some(version >> 16);
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
        })
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
    /// [`HalError::ShaderCompilation`] naming both stages.
    pub(crate) fn expect(&self, stage: ShaderStages, entry_point: &str) -> Result<(), HalError> {
        let wanted = match stage {
            ShaderStages::VERTEX => KIND_VERTEX,
            ShaderStages::FRAGMENT => KIND_PIXEL,
            _ => KIND_COMPUTE,
        };
        if self.kind == wanted {
            return Ok(());
        }
        Err(HalError::ShaderCompilation(format!(
            "the module bound as `{entry_point}` for the {} stage holds a {} container; a DXIL \
             container is compiled for one entry point at one stage, so this is the wrong \
             artifact rather than the wrong entry-point name",
            stage_name(stage),
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

/// How a message names a stage.
const fn stage_name(stage: ShaderStages) -> &'static str {
    match stage {
        ShaderStages::VERTEX => "vertex",
        ShaderStages::FRAGMENT => "fragment",
        _ => "compute",
    }
}

/// How a message names a container's own shader kind.
const fn stage_name_of_kind(kind: u32) -> &'static str {
    match kind {
        KIND_PIXEL => "pixel",
        KIND_VERTEX => "vertex",
        KIND_COMPUTE => "compute",
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

/// The next free register in each class, as `dxc` counts them.
///
/// One of these is threaded through a whole pipeline layout — see the module
/// docs on why the count does not restart per set.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Registers {
    cbv: u32,
    srv: u32,
    uav: u32,
    sampler: u32,
}

impl Registers {
    /// Takes `count` consecutive registers in `class` and returns the first.
    ///
    /// `count` is a *root signature* range length, so an unbounded range arrives
    /// as [`u32::MAX`]: it consumes the rest of its class, and the saturating
    /// add is what makes a binding declared after one land somewhere legal
    /// rather than wrapping back onto a register the unbounded range covers.
    pub(crate) const fn take(&mut self, class: RegisterClass, count: u32) -> u32 {
        let slot = match class {
            RegisterClass::Cbv => &mut self.cbv,
            RegisterClass::Srv => &mut self.srv,
            RegisterClass::Uav => &mut self.uav,
            RegisterClass::Sampler => &mut self.sampler,
        };
        let base = *slot;
        *slot = slot.saturating_add(count);
        base
    }
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
    #[test]
    fn the_committed_graphics_shaders_resolve_both_stages() {
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
            for (stage, stages) in [
                (crcbl_shaders::Stage::Vertex, ShaderStages::VERTEX),
                (crcbl_shaders::Stage::Fragment, ShaderStages::FRAGMENT),
            ] {
                let entry_point = shader
                    .entry_point(stage)
                    .unwrap_or_else(|| panic!("{}: no {stage:?} entry point", shader.name()));
                entry
                    .container(entry_point)
                    .unwrap_or_else(|error| panic!("{}: {error}", shader.name()))
                    .expect(stages, entry_point)
                    .unwrap_or_else(|error| panic!("{}: {error}", shader.name()));
            }
        }
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
        let compute: &[(&str, &crcbl_shaders::Shader, u32)] = &[
            (
                "compute_probe",
                &crcbl_shaders::COMPUTE_PROBE,
                crcbl_shaders::compute_probe::WORKGROUP_SIZE,
            ),
            (
                "cull",
                &crcbl_shaders::CULL,
                crcbl_shaders::cull::WORKGROUP_SIZE,
            ),
            (
                "draw_gen",
                &crcbl_shaders::DRAW_GEN,
                crcbl_shaders::draw_gen::WORKGROUP_SIZE,
            ),
        ];
        assert!(!compute.is_empty(), "nothing to check");
        for (name, shader, size) in compute {
            let bytes = shader
                .dxil("computeMain")
                .unwrap_or_else(|| panic!("{name} commits a DXIL container"));
            let parsed = Dxil::parse(bytes, name).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(
                parsed.numthreads(),
                Some([*size, 1, 1]),
                "{name}'s container disagrees with WORKGROUP_SIZE"
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

    /// **The register each binding lands on is what the committed container
    /// declares**, checked against every shipped artifact's own resource table.
    ///
    /// The rule under test is the module docs': per class, in ascending
    /// `(set, binding)` order, across the whole source. It matters because a
    /// root signature whose ranges name registers the shader does not use is
    /// rejected by pipeline creation — on a Windows runner, one CI round trip
    /// away from here.
    ///
    /// Every shader is listed, not only the compute ones, because the mistake
    /// this replaced was invisible for exactly the shaders whose sets hold one
    /// resource class. `sprite` earns its place twice over: its bindings are
    /// spread across **two** sets, and the container numbers them end to end in
    /// space 0 — which is what the running counter reproduces and a per-set one
    /// could not.
    #[test]
    fn registers_are_assigned_per_class_in_declaration_order() {
        use RegisterClass::{Cbv, Sampler, Srv, Uav};

        // `(shader, its entry points, the classes its bindings take in
        // ascending (set, binding) order)`, transcribed from the `.slang`
        // sources' declarations. Every entry point is named because `dxc`
        // numbers across the whole source while a container records only what
        // its own entry point reached, so the union is what the source declares.
        let cases: &[(&str, &crcbl_shaders::Shader, &[&str], &[RegisterClass])] = &[
            (
                "compute_probe",
                &crcbl_shaders::COMPUTE_PROBE,
                &["computeMain"],
                &[Cbv, Srv, Uav],
            ),
            (
                "cull",
                &crcbl_shaders::CULL,
                &["computeMain"],
                &[Cbv, Srv, Srv, Uav, Uav],
            ),
            (
                "draw_gen",
                &crcbl_shaders::DRAW_GEN,
                &["computeMain"],
                &[Cbv, Srv, Srv, Srv, Srv, Srv, Uav, Uav, Uav],
            ),
            (
                "mesh",
                &crcbl_shaders::MESH,
                &["vertexMain", "fragmentMain"],
                &[Cbv, Srv, Srv, Cbv, Srv, Srv],
            ),
            (
                "sprite",
                &crcbl_shaders::SPRITE,
                &["vertexMain", "fragmentMain"],
                // Set 0 then set 1: the sheet is `t1` in space 0, not `t0` in
                // space 1.
                &[Cbv, Srv, Srv, Sampler],
            ),
            (
                "tonemap",
                &crcbl_shaders::TONEMAP,
                &["vertexMain", "fragmentMain"],
                &[Srv, Sampler],
            ),
            (
                "triangle",
                &crcbl_shaders::TRIANGLE,
                &["vertexMain", "fragmentMain"],
                &[Srv],
            ),
            (
                "ui",
                &crcbl_shaders::UI,
                &["vertexMain", "fragmentMain"],
                &[Srv, Sampler, Srv, Cbv],
            ),
        ];
        assert!(!cases.is_empty(), "nothing to check");
        for (name, shader, entry_points, classes) in cases {
            assert!(!entry_points.is_empty(), "{name}: no entry point to read");
            let mut declared: Vec<(RegisterClass, u32)> = Vec::new();
            for entry_point in *entry_points {
                let bytes = shader.dxil(entry_point).unwrap_or_else(|| {
                    panic!("{name} commits a DXIL container for `{entry_point}`")
                });
                for binding in resource_table(bytes) {
                    if !declared.contains(&binding) {
                        declared.push(binding);
                    }
                }
            }
            declared.sort_unstable();

            let mut registers = Registers::default();
            let mut assigned: Vec<(RegisterClass, u32)> = classes
                .iter()
                .map(|class| (*class, registers.take(*class, 1)))
                .collect();
            assigned.sort_unstable();
            // The whole *set*, not membership one at a time: a counter that
            // never advanced would hand every binding of a class register zero,
            // and every one of those is a register the container really
            // declares — so only comparing the collections catches it.
            assert_eq!(
                assigned, declared,
                "{name}: the registers this backend would put in a root signature are not the \
                 ones the container declares"
            );
        }
    }

    /// A container's `PSV0` resource table: what each binding is, and where.
    ///
    /// Test-only, because the runtime path never needs it: the root signature is
    /// built from the caller's declared layout, and this is how that layout is
    /// checked against the artifact rather than a second source for it.
    ///
    /// The table follows the runtime info: a `u32` count, a `u32` stride, then
    /// that many records, each opening `ResType`, `Space`, `LowerBound`,
    /// `UpperBound`. `PSVResourceType` numbers sampler 1, CBV 2, the three SRV
    /// kinds 3–5 and the UAV kinds from 6.
    fn resource_table(bytes: &[u8]) -> Vec<(RegisterClass, u32)> {
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
                let space = word(bytes, record + 4).expect("a register space");
                assert_eq!(
                    space, 0,
                    "Slang's HLSL output puts every resource in space 0; this one is in {space}"
                );
                (class, word(bytes, record + 8).expect("a lower bound"))
            })
            .collect()
    }

    /// Each class counts on its own, and an unbounded range takes the rest of
    /// its own.
    #[test]
    fn each_register_class_counts_independently() {
        let mut registers = Registers::default();
        assert_eq!(registers.take(RegisterClass::Cbv, 1), 0);
        assert_eq!(
            registers.take(RegisterClass::Srv, 1),
            0,
            "t restarts at zero"
        );
        assert_eq!(registers.take(RegisterClass::Srv, 2), 1);
        assert_eq!(registers.take(RegisterClass::Srv, 1), 3);
        assert_eq!(registers.take(RegisterClass::Uav, 1), 0, "u restarts too");
        assert_eq!(registers.take(RegisterClass::Sampler, 1), 0);

        // An unbounded range consumes the rest of its class, and the next
        // binding of that class lands at the ceiling rather than wrapping onto
        // a register the unbounded range already covers.
        let mut unbounded = Registers::default();
        assert_eq!(unbounded.take(RegisterClass::Srv, u32::MAX), 0);
        assert_eq!(unbounded.take(RegisterClass::Srv, 1), u32::MAX);
    }
}
