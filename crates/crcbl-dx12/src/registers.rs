//! **Every register a pipeline's containers declare is one its layout puts a
//! binding of that kind at**, checked when the pipeline is created, before
//! D3D12 is handed either half.
//!
//! D3D12's own answer to a disagreement is no answer. A root signature that
//! does not cover a register a stage declares fails `CreateGraphicsPipelineState`
//! with `E_INVALIDARG` and a message naming neither the register nor the
//! binding. A root signature that covers it with the wrong *kind* of descriptor
//! is accepted, and the stage reads a plausible, wrong resource. So both halves
//! are compared here, where both are in hand: each stage's `PSV0` resource table
//! — read once, when the shader module is created, by `crate::dxil` — against
//! the registers `crate::root::assign_registers` gave the pipeline layout's
//! bindings.
//!
//! The case this is written for is DXIL a game compiles itself. A source that
//! declares no `register(…)` is numbered by `dxc` per register class, each from
//! zero — `t0`, `s0`, `b0`, `t1` — while this backend puts every binding at its
//! binding number in the space its set names. The two agree only by accident,
//! and the refusal says which resource moved and where the layout has it.
//!
//! One comparison with two callers: `crate::pipeline` at every pipeline
//! creation, and `crate::renderer_registers`, which holds the renderer's own
//! layouts to their containers on any host.
//!
//! # What it costs
//!
//! Nothing per draw. The table is parsed once per container at
//! `create_shader_module`, the layout's registers once per
//! `create_pipeline_layout`, and each pipeline creation scans one against the
//! other — a handful of records against a few dozen bindings, beside a
//! pipeline state object compile.

use crcbl_hal::{BindGroupLayoutEntry, BindingFlags, BindingKind, HalError, ImageViewType, Limits};

use crate::dxil::{PsvResource, RegisterClass};
use crate::root;

/// Where one layout binding lands in a root signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlacedBinding {
    /// The register space, which is also the set's index — see
    /// [`root::space_of`].
    pub(crate) space: u32,
    /// The seam's binding number.
    pub(crate) binding: u32,
    /// What the layout declares there.
    pub(crate) kind: BindingKind,
    /// The register file [`root::class_of`] puts `kind` in.
    pub(crate) class: RegisterClass,
    /// The register [`root::assign_registers`] gave it.
    pub(crate) register: u32,
    /// `NumDescriptors`: the resolved count, or `u32::MAX` for an unbounded
    /// range.
    pub(crate) declared: u32,
}

impl PlacedBinding {
    /// This binding as a message names it.
    fn describe(&self) -> String {
        format!(
            "layout set {} binding {} ({}) is {}{} space{}",
            self.space,
            self.binding,
            kind_name(self.kind),
            letter(self.class),
            self.register,
            self.space
        )
    }
}

/// Every binding of one set, placed in `space` by the rule every root signature
/// this crate builds follows.
///
/// `limits` resolves a `u32::MAX` count, as
/// [`BindGroupLayoutEntry::resolved_count`] does for the range itself.
pub(crate) fn place_set(
    entries: &[BindGroupLayoutEntry],
    space: u32,
    limits: &Limits,
) -> Vec<PlacedBinding> {
    let bindings: Vec<root::Binding> = entries
        .iter()
        .map(|entry| root::Binding {
            binding: entry.binding,
            class: root::class_of(entry.kind),
            declared: if entry.flags.contains(BindingFlags::VARIABLE_COUNT) {
                u32::MAX
            } else {
                entry.resolved_count(limits)
            },
        })
        .collect();
    let registers = root::assign_registers(&bindings);
    entries
        .iter()
        .zip(bindings)
        .zip(registers)
        .map(|((entry, binding), register)| PlacedBinding {
            space,
            binding: entry.binding,
            kind: entry.kind,
            class: binding.class,
            register,
            declared: binding.declared,
        })
        .collect()
}

/// Every register one pipeline layout's root signature declares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayoutRegisters {
    /// Every binding of every set, in set order.
    pub(crate) bindings: Vec<PlacedBinding>,
    /// The push-constant block's `(space, register)`, when the layout declares
    /// a range — always `b0` in [`root::PUSH_CONSTANT_SPACE`].
    pub(crate) push_constants: Option<(u32, u32)>,
}

impl LayoutRegisters {
    /// Every resource in `resources` the layout does not hold, as clauses that
    /// each open with "declares" and name both sides.
    ///
    /// A binding the layout declares and the table does not name is not a
    /// disagreement: one layout serves every stage, and each container declares
    /// only what its entry point reads.
    pub(crate) fn disagreements(&self, resources: &[PsvResource]) -> Vec<String> {
        let mut found = Vec::new();
        for resource in resources {
            let PsvResource {
                class,
                space,
                lower,
                upper,
                kind,
            } = *resource;
            if class == RegisterClass::Cbv && self.push_constants == Some((space, lower)) {
                continue;
            }
            let declares = format!(
                "declares {} {}{lower} space{space}",
                class_name(class),
                letter(class)
            );
            let Some(binding) = self.bindings.iter().find(|binding| {
                binding.class == class && binding.space == space && binding.register == lower
            }) else {
                found.push(self.missing(class, space, lower, &declares));
                continue;
            };
            if let Some(kind) = kind
                && !accepts(binding.kind, kind)
            {
                found.push(format!(
                    "{declares} as a {}; {}, which cannot stand there",
                    psv_kind_name(kind),
                    binding.describe()
                ));
            }
            let extent = u64::from(upper) - u64::from(lower) + 1;
            if binding.declared != u32::MAX && extent > u64::from(binding.declared) {
                found.push(format!(
                    "{declares} and the {} registers after it; {} declares {} descriptor(s)",
                    extent - 1,
                    binding.describe(),
                    binding.declared
                ));
            }
        }
        found
    }

    /// Refuses a stage whose resource table the layout does not hold.
    ///
    /// # Errors
    ///
    /// [`HalError::ShaderCompilation`] naming `pipeline`, `entry_point`, each
    /// resource the container declares that the layout does not hold — class,
    /// register and space — what the layout has at that binding and of that
    /// class, and the rule a shader must follow.
    pub(crate) fn check(
        &self,
        resources: &[PsvResource],
        pipeline: &str,
        entry_point: &str,
    ) -> Result<(), HalError> {
        let found = self.disagreements(resources);
        if found.is_empty() {
            return Ok(());
        }
        Err(HalError::ShaderCompilation(format!(
            "`{pipeline}` {entry_point} {}; shaders must declare register(<class><binding>, \
             space<set>), and a push-constant block register(b0, space{})",
            found.join("; and "),
            root::PUSH_CONSTANT_SPACE
        )))
    }

    /// The clause for a resource at a register the layout puts nothing of its
    /// class at: what the layout has at that binding number, and every binding
    /// of that class in that set — where a shader numbered per class meant to
    /// point.
    fn missing(&self, class: RegisterClass, space: u32, register: u32, declares: &str) -> String {
        if class == RegisterClass::Cbv
            && space == root::PUSH_CONSTANT_SPACE
            && self.push_constants.is_none()
        {
            return format!(
                "{declares}, the push-constant block's register, and the layout declares no \
                 push-constant range"
            );
        }
        let nearby: Vec<String> = self
            .bindings
            .iter()
            .filter(|binding| {
                binding.space == space && (binding.binding == register || binding.class == class)
            })
            .map(PlacedBinding::describe)
            .collect();
        if nearby.is_empty() {
            return format!(
                "{declares}; layout set {space} has no binding {register} and no {} binding",
                class_name(class)
            );
        }
        format!("{declares}; {}", nearby.join("; "))
    }
}

/// The HLSL register letter of `class`.
const fn letter(class: RegisterClass) -> char {
    match class {
        RegisterClass::Cbv => 'b',
        RegisterClass::Srv => 't',
        RegisterClass::Uav => 'u',
        RegisterClass::Sampler => 's',
    }
}

/// How a message names `class`.
const fn class_name(class: RegisterClass) -> &'static str {
    match class {
        RegisterClass::Cbv => "CBV",
        RegisterClass::Srv => "SRV",
        RegisterClass::Uav => "UAV",
        RegisterClass::Sampler => "sampler",
    }
}

/// How a message names a binding kind: its variant, without the fields.
const fn kind_name(kind: BindingKind) -> &'static str {
    match kind {
        BindingKind::UniformBuffer { .. } => "UniformBuffer",
        BindingKind::StorageBuffer { .. } => "StorageBuffer",
        BindingKind::SampledImage { .. } => "SampledImage",
        BindingKind::StorageImage { .. } => "StorageImage",
        BindingKind::Sampler { .. } => "Sampler",
    }
}

// `PSVResourceKind`, the container format's: what the shader declared a
// resource as. Fixed outside this repository.
const TEXTURE_1D: u32 = 1;
const TEXTURE_2D: u32 = 2;
const TEXTURE_3D: u32 = 4;
const TEXTURE_CUBE: u32 = 5;
const TEXTURE_2D_ARRAY: u32 = 7;
const TEXTURE_CUBE_ARRAY: u32 = 9;
const RAW_BUFFER: u32 = 11;
const STRUCTURED_BUFFER: u32 = 12;
const CBUFFER: u32 = 13;
const SAMPLER: u32 = 14;

/// How a message names a `PSVResourceKind`.
fn psv_kind_name(kind: u32) -> String {
    match kind {
        TEXTURE_1D => "Texture1D".to_owned(),
        TEXTURE_2D => "Texture2D".to_owned(),
        TEXTURE_3D => "Texture3D".to_owned(),
        TEXTURE_CUBE => "TextureCube".to_owned(),
        TEXTURE_2D_ARRAY => "Texture2DArray".to_owned(),
        TEXTURE_CUBE_ARRAY => "TextureCubeArray".to_owned(),
        RAW_BUFFER => "raw buffer".to_owned(),
        STRUCTURED_BUFFER => "structured buffer".to_owned(),
        CBUFFER => "cbuffer".to_owned(),
        SAMPLER => "sampler".to_owned(),
        other => format!("PSVResourceKind {other}"),
    }
}

/// Whether a binding of `kind` can stand where the shader declared a resource
/// of `PSVResourceKind` `resource`.
const fn accepts(kind: BindingKind, resource: u32) -> bool {
    const fn dimension(view_type: ImageViewType) -> u32 {
        match view_type {
            ImageViewType::D1 => TEXTURE_1D,
            ImageViewType::D2 => TEXTURE_2D,
            ImageViewType::D2Array => TEXTURE_2D_ARRAY,
            ImageViewType::Cube => TEXTURE_CUBE,
            ImageViewType::CubeArray => TEXTURE_CUBE_ARRAY,
            ImageViewType::D3 => TEXTURE_3D,
        }
    }
    match kind {
        BindingKind::UniformBuffer { .. } => resource == CBUFFER,
        BindingKind::StorageBuffer { .. } => {
            resource == STRUCTURED_BUFFER || resource == RAW_BUFFER
        }
        BindingKind::SampledImage { view_type, .. }
        | BindingKind::StorageImage { view_type, .. } => resource == dimension(view_type),
        BindingKind::Sampler { .. } => resource == SAMPLER,
    }
}

#[cfg(test)]
mod tests {
    use crcbl_hal::{SampleType, ShaderStages};

    use super::*;
    use crate::dxil::{Dxil, KIND_PIXEL, test_container};

    /// A record of a container's resource table.
    fn record(class: RegisterClass, space: u32, register: u32, kind: u32) -> PsvResource {
        PsvResource {
            class,
            space,
            lower: register,
            upper: register,
            kind: Some(kind),
        }
    }

    /// One scalar binding visible to the fragment stage.
    fn entry(binding: u32, kind: BindingKind) -> BindGroupLayoutEntry {
        BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            kind,
            count: 1,
            flags: BindingFlags::empty(),
        }
    }

    /// The shape EW's `scope.slang` is used with: a texture, its sampler, a
    /// uniform block and a second texture at bindings 0 to 3 of set 0, and a
    /// push-constant block.
    fn scope_layout() -> LayoutRegisters {
        let texture = BindingKind::SampledImage {
            view_type: ImageViewType::D2,
            sample_type: SampleType::Float,
        };
        let entries = [
            entry(0, texture),
            entry(1, BindingKind::Sampler { comparison: false }),
            entry(2, BindingKind::UniformBuffer { dynamic: false }),
            entry(3, texture),
        ];
        LayoutRegisters {
            bindings: place_set(&entries, 0, &Limits::desktop()),
            push_constants: Some((root::PUSH_CONSTANT_SPACE, 0)),
        }
    }

    /// `layout`'s verdict on a fragment container declaring `resources`, read
    /// through the parser and the check the pipeline path runs.
    fn verdict(layout: &LayoutRegisters, resources: &[PsvResource]) -> Result<(), HalError> {
        let bytes = test_container(KIND_PIXEL, Some(resources));
        Dxil::parse(&bytes, "scope")
            .expect("a well-formed container")
            .require_registers(layout, "scope lens", "fragmentMain")
    }

    /// **A container `dxc` numbered per register class is refused, naming each
    /// resource that moved and where the layout has it.**
    ///
    /// The source declared no `register(…)`, so `dxc` gave the texture, the
    /// sampler, the uniform block and the second texture `t0`, `s0`, `b0` and
    /// `t1` — which the counting rule this backend used to follow agreed with.
    /// The layout puts them at `t0`, `s1`, `b2` and `t3`, so `t0` alone agrees,
    /// and each of the other three is named with what the layout has instead.
    #[test]
    fn a_container_numbered_per_class_is_refused_naming_each_resource() {
        let error = verdict(
            &scope_layout(),
            &[
                record(RegisterClass::Srv, 0, 0, TEXTURE_2D),
                record(RegisterClass::Sampler, 0, 0, SAMPLER),
                record(RegisterClass::Cbv, 0, 0, CBUFFER),
                record(RegisterClass::Srv, 0, 1, TEXTURE_2D),
            ],
        )
        .expect_err("three of the four registers are not where the layout puts them");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("a register disagreement is not {error:?}");
        };
        for expected in [
            "`scope lens` fragmentMain declares sampler s0 space0; layout set 0 binding 0 \
             (SampledImage) is t0 space0; layout set 0 binding 1 (Sampler) is s1 space0",
            "declares CBV b0 space0; layout set 0 binding 0 (SampledImage) is t0 space0; layout \
             set 0 binding 2 (UniformBuffer) is b2 space0",
            "declares SRV t1 space0; layout set 0 binding 0 (SampledImage) is t0 space0; layout \
             set 0 binding 1 (Sampler) is s1 space0; layout set 0 binding 3 (SampledImage) is t3 \
             space0",
            "shaders must declare register(<class><binding>, space<set>), and a push-constant \
             block register(b0, space64)",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in: {text}");
        }
        assert!(
            !text.contains("declares SRV t0"),
            "t0 is where the layout puts binding 0, so it is not a disagreement: {text}"
        );
    }

    /// **The same resources at the registers the layout assigns pass**, and so
    /// does a container declaring only some of them beside a push-constant
    /// block — one layout serves every stage, and each stage declares only what
    /// it reads.
    #[test]
    fn a_container_at_the_layouts_registers_passes() {
        let layout = scope_layout();
        verdict(
            &layout,
            &[
                record(RegisterClass::Srv, 0, 0, TEXTURE_2D),
                record(RegisterClass::Sampler, 0, 1, SAMPLER),
                record(RegisterClass::Cbv, 0, 2, CBUFFER),
                record(RegisterClass::Srv, 0, 3, TEXTURE_2D),
            ],
        )
        .expect("every register is the binding number in the set's space");
        verdict(
            &layout,
            &[
                record(RegisterClass::Srv, 0, 3, TEXTURE_2D),
                record(RegisterClass::Cbv, root::PUSH_CONSTANT_SPACE, 0, CBUFFER),
            ],
        )
        .expect("a subset, and the push-constant block at b0 space64");
        verdict(&layout, &[]).expect("a stage that reads nothing");
    }

    /// **A push-constant block needs a push-constant range**, a set the layout
    /// does not have is named as missing, and a resource of the right class at
    /// the right register but the wrong kind is refused as one.
    #[test]
    fn a_missing_range_a_missing_set_and_a_wrong_kind_are_each_named() {
        let push = record(RegisterClass::Cbv, root::PUSH_CONSTANT_SPACE, 0, CBUFFER);
        let without_range = LayoutRegisters {
            push_constants: None,
            ..scope_layout()
        };
        let text = verdict(&without_range, &[push])
            .expect_err("no range, so no root constants at b0 space64")
            .to_string();
        assert!(
            text.contains(
                "declares CBV b0 space64, the push-constant block's register, and the layout \
                 declares no push-constant range"
            ),
            "{text}"
        );

        let text = verdict(
            &scope_layout(),
            &[record(RegisterClass::Srv, 1, 0, TEXTURE_2D)],
        )
        .expect_err("the layout has one set")
        .to_string();
        assert!(
            text.contains(
                "declares SRV t0 space1; layout set 1 has no binding 0 and no SRV binding"
            ),
            "{text}"
        );

        let text = verdict(
            &scope_layout(),
            &[record(RegisterClass::Srv, 0, 3, STRUCTURED_BUFFER)],
        )
        .expect_err("a structured buffer where the layout has a texture")
        .to_string();
        assert!(
            text.contains(
                "declares SRV t3 space0 as a structured buffer; layout set 0 binding 3 \
                 (SampledImage) is t3 space0, which cannot stand there"
            ),
            "{text}"
        );
    }

    /// A container with no resource table says it cannot be checked, rather
    /// than passing a check it could not make.
    #[test]
    fn a_container_without_a_resource_table_is_refused() {
        let bytes = test_container(KIND_PIXEL, None);
        let error = Dxil::parse(&bytes, "scope")
            .expect("a well-formed container")
            .require_registers(&scope_layout(), "scope lens", "fragmentMain")
            .expect_err("nothing to check against");
        let HalError::ShaderCompilation(text) = &error else {
            panic!("{error:?}");
        };
        assert!(text.contains("no PSV0 part"), "{text}");
    }
}
