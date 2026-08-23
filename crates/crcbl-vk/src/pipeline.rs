//! Shader modules, descriptor layouts, bind groups, samplers and pipelines —
//! milestone 2 of `docs/plan/02-vulkan-backend.md`'s ladder.
//!
//! # No vertex input state, anywhere
//!
//! [`vk::PipelineVertexInputStateCreateInfo`] is created empty and stays empty.
//! That is not a simplification: `crcbl-hal`'s pipeline module has no
//! vertex-buffer layout and there is no `bind_vertex_buffer`, because **vertex
//! pulling is the only geometry path** (`docs/plan/03-gpu-driven-rendering.md`
//! §3.1: "Vertex pulling everywhere (established in stage 2): position/attr
//! streams as storage buffers, no pipeline vertex-input state to vary per
//! mesh"). A pipeline that varied with the mesh is the thing that stops ten
//! objects and ten thousand objects recording the same commands.
//!
//! # Dynamic rendering, so there is no render pass to be compatible with
//!
//! Pipelines declare attachment *formats* through
//! [`vk::PipelineRenderingCreateInfo`], never a `VkRenderPass`. The seam is
//! shaped that way on purpose, and it is why
//! [`GraphicsPipelineDesc`] carries
//! `color_targets` while the attachments themselves arrive at
//! `begin_render_pass`.
//!
//! # One descriptor pool per bind group
//!
//! Coarse, and deliberately so for P1.2. A `VkDescriptorPool` sized exactly for
//! one set cannot fragment, cannot run out, and cannot be recycled wrong — and
//! recycling is what `docs/plan/02-vulkan-backend.md` §2.2 wants ("Per-frame:
//! … descriptor recycling"), which needs the frame ring the render graph will
//! own at P1.3. Guessing that ring's shape here, to save a driver allocation on
//! a path that runs a handful of times at startup, is the wrong trade. It is
//! the same argument `command.rs` makes for one command pool per encoder.
//!
//! # Bindless is declared here and exercised at P3
//!
//! [`BindingFlags`] is translated in full —
//! `PARTIALLY_BOUND`, `UPDATE_AFTER_BIND` and `VARIABLE_COUNT` all reach the
//! driver — and a Tier B device **rejects** them rather than ignoring them,
//! which is what the seam requires: "a bindless array quietly downgraded to a
//! fixed one reads garbage at index 4097". What is not here is a bindless array
//! at scale; that is topic 03's, at P3.

use std::sync::Arc;

use ash::vk;

use crcbl_hal::{
    BindGroupDesc, BindGroupEntry, BindGroupHandle, BindGroupLayoutDesc, BindGroupLayoutEntry,
    BindGroupLayoutHandle, BindingFlags, BindingKind, BindingResource, ComputePipelineDesc,
    ComputePipelineHandle, DeviceCaps, Features, GraphicsPipelineDesc, GraphicsPipelineHandle,
    HalError, PipelineLayoutDesc, PipelineLayoutHandle, SamplerDesc, SamplerHandle, ShaderEntry,
    ShaderModuleDesc, ShaderModuleHandle, ShaderSources, ShaderStages,
};

use crate::conv;
use crate::device::{DeviceInner, DeviceState, VkDevice, lookup};
use crate::spirv;

/// A compiled shader module, and what it contains.
#[derive(Debug)]
pub(crate) struct ShaderModuleEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::ShaderModule,
    /// The module's own words, kept so pipeline creation can check an entry
    /// point *before* the driver does. A few kilobytes per module, once.
    pub(crate) words: Vec<u32>,
}

/// A descriptor-set layout, plus everything a bind group needs to know about
/// it that Vulkan will not hand back.
#[derive(Debug)]
pub(crate) struct BindGroupLayoutEntryRecord {
    pub(crate) owner: u64,
    pub(crate) raw: vk::DescriptorSetLayout,
    /// The declared bindings, in the order the caller gave them.
    pub(crate) entries: Vec<BindGroupLayoutEntry>,
    /// Whether any binding is `UPDATE_AFTER_BIND`, which the pool must also be
    /// created with — a mismatch is `VUID-vkAllocateDescriptorSets-pSetLayouts-03044`.
    pub(crate) update_after_bind: bool,
    /// The binding number carrying `VARIABLE_COUNT`, if any.
    pub(crate) variable_binding: Option<u32>,
}

/// A descriptor set and the pool it was allocated from.
#[derive(Debug)]
pub(crate) struct BindGroupEntryRecord {
    pub(crate) owner: u64,
    pub(crate) raw: vk::DescriptorSet,
    /// Freed as a unit; see the module docs on one pool per group.
    pub(crate) pool: vk::DescriptorPool,
    pub(crate) layout: BindGroupLayoutHandle,
    /// Whether the layout allows writes while command buffers are pending.
    pub(crate) update_after_bind: bool,
}

/// A pipeline layout.
#[derive(Debug)]
pub(crate) struct PipelineLayoutEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::PipelineLayout,
    /// Push-constant stages, so `push_constants` can pass the right mask
    /// without the caller's `stages` having to agree exactly.
    pub(crate) push_constant_stages: vk::ShaderStageFlags,
    pub(crate) push_constant_size: u32,
}

/// A graphics or compute pipeline.
#[derive(Debug)]
pub(crate) struct PipelineEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::Pipeline,
}

/// A sampler.
#[derive(Debug)]
pub(crate) struct SamplerEntry {
    pub(crate) owner: u64,
    pub(crate) raw: vk::Sampler,
}

// --- pure validation -------------------------------------------------------

/// How many descriptors to allocate for a `VARIABLE_COUNT` binding.
///
/// The seam now carries a [`BindGroupDesc::variable_count`] field.
/// When `Some(n)`, the backend uses `n` directly rather than inferring
/// from entries — which is what the bindless model needs (allocate N
/// slots, write a subset today, stream the rest later through
/// [`Device::update_bind_group`](crcbl_hal::Device::update_bind_group)).
/// When `None`, the inference below applies.
///
/// This function is the fallback: the highest slot written, plus one
/// (minimum 1). Safe for fixed-size arrays; wrong for deferred fill.
#[must_use]
pub(crate) fn variable_count_from_entries(binding: u32, entries: &[BindGroupEntry]) -> u32 {
    entries
        .iter()
        .filter(|entry| entry.binding == binding)
        .map(|entry| entry.array_index.saturating_add(1))
        .max()
        .unwrap_or(1)
}

// --- creation --------------------------------------------------------------

impl VkDevice {
    pub(crate) fn create_shader_module_impl(
        &self,
        desc: &ShaderModuleDesc<'_>,
    ) -> Result<ShaderModuleHandle, HalError> {
        // Vulkan consumes SPIR-V and nothing else. `desc.wgsl` is ignored
        // rather than translated: a WGSL→SPIR-V frontend inside the Vulkan
        // backend would be a second shader compiler in the hot path to fix a
        // problem no Vulkan driver has, and the engine's SPIR-V is the artifact
        // this backend was designed around — including the `DrawParameters`
        // capability that vertex pulling needs and that naga cannot read.
        if desc.spirv.is_empty() {
            return Err(desc.unusable(ShaderSources::SPIRV));
        }

        // Parsed before the driver sees it, so "these are bytes, not words" and
        // "this module has no entry points" are reported by name rather than as
        // a driver error at pipeline creation.
        let entry_points = spirv::entry_points(desc.spirv).map_err(HalError::ShaderCompilation)?;
        if entry_points.is_empty() {
            return Err(HalError::ShaderCompilation(
                "the module declares no OpEntryPoint, so no pipeline could ever use it".to_string(),
            ));
        }

        let info = vk::ShaderModuleCreateInfo::default().code(desc.spirv);
        // SAFETY: `desc.spirv` is a `&[u32]`, so it is word-aligned and its
        // length is a whole number of words — the two things
        // `vkCreateShaderModule` requires of `pCode`/`codeSize`. The magic
        // number and instruction stream were checked above.
        let raw = unsafe { self.inner().raw.create_shader_module(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateShaderModule", error))?;
        self.inner().set_object_name(raw, desc.label);

        Ok(self.inner().stamp(
            self.inner()
                .state()
                .shader_modules
                .insert(ShaderModuleEntry {
                    owner: self.inner().id,
                    raw,
                    words: desc.spirv.to_vec(),
                }),
        ))
    }

    pub(crate) fn create_bind_group_layout_impl(
        &self,
        desc: &BindGroupLayoutDesc<'_>,
    ) -> Result<BindGroupLayoutHandle, HalError> {
        let caps = self.inner().caps;
        desc.check_entries(&caps, crcbl_hal::BackendKind::Vulkan)?;

        let bindings: Vec<vk::DescriptorSetLayoutBinding<'_>> = desc
            .entries
            .iter()
            .map(|entry| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(entry.binding)
                    .descriptor_type(conv::descriptor_type(entry.kind))
                    .descriptor_count(entry.resolved_count(&caps.limits))
                    .stage_flags(conv::shader_stages(entry.visibility))
            })
            .collect();
        let flags: Vec<vk::DescriptorBindingFlags> = desc
            .entries
            .iter()
            .map(|entry| conv::binding_flags(entry.flags))
            .collect();

        let update_after_bind = desc
            .entries
            .iter()
            .any(|entry| entry.flags.contains(BindingFlags::UPDATE_AFTER_BIND));
        let variable_binding = desc
            .entries
            .iter()
            .find(|entry| entry.flags.contains(BindingFlags::VARIABLE_COUNT))
            .map(|entry| entry.binding);

        let mut flags_info =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&flags);
        let mut info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        if update_after_bind {
            info = info.flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL);
        }
        // Only chained when something actually asked for it: an all-empty flags
        // array is legal but pointless, and older drivers are happier without.
        if desc.entries.iter().any(|entry| !entry.flags.is_empty()) {
            info = info.push_next(&mut flags_info);
        }

        // SAFETY: `info` borrows `bindings` and `flags`, both of which outlive
        // the call, and every count and flag was checked above.
        let raw = unsafe { self.inner().raw.create_descriptor_set_layout(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateDescriptorSetLayout", error))?;
        self.inner().set_object_name(raw, desc.label);

        Ok(self
            .inner()
            .stamp(
                self.inner()
                    .state()
                    .bind_group_layouts
                    .insert(BindGroupLayoutEntryRecord {
                        owner: self.inner().id,
                        raw,
                        entries: desc.entries.to_vec(),
                        update_after_bind,
                        variable_binding,
                    }),
            ))
    }

    pub(crate) fn create_bind_group_impl(
        &self,
        desc: &BindGroupDesc<'_>,
    ) -> Result<BindGroupHandle, HalError> {
        let caps = self.inner().caps;
        let inner = Arc::clone(self.inner());
        let mut state = inner.state();

        let layout = lookup(
            &state.bind_group_layouts,
            "bind group layout",
            desc.layout,
            &inner,
        )?;
        let layout_raw = layout.raw;
        let update_after_bind = layout.update_after_bind;
        let variable_binding = layout.variable_binding;
        let entries = layout.entries.clone();

        // Every entry must name a binding the layout declares, or the write
        // below is a validation error the driver reports without saying which
        // of the caller's entries produced it.
        for entry in desc.entries {
            let Some(declared) = entries.iter().find(|slot| slot.binding == entry.binding) else {
                return Err(HalError::InvalidDescriptor(format!(
                    "bind group entry names binding {}, which the layout does not declare",
                    entry.binding
                )));
            };
            let limit = if Some(entry.binding) == variable_binding {
                declared.resolved_count(&caps.limits)
            } else {
                declared.count
            };
            if entry.array_index >= limit {
                return Err(HalError::InvalidDescriptor(format!(
                    "bind group entry writes index {} of binding {}, which holds {limit}",
                    entry.array_index, entry.binding
                )));
            }
            check_resource_kind(declared.kind, &entry.resource, entry.binding)?;
        }

        // `variable_count` sizes the pool *and* fills
        // `VkDescriptorSetVariableDescriptorCountAllocateInfo`, so an over-large
        // one used to fail `vkAllocateDescriptorSets` with
        // `VUID-VkDescriptorSetVariableDescriptorCountAllocateInfo-pSetLayouts-03046`
        // — a driver message naming neither the caller's binding nor the
        // ceiling it broke. Checked here instead, in the vocabulary the rest of
        // the module produces.
        if let Some(count) = desc.variable_count {
            let Some(binding) = variable_binding else {
                return Err(HalError::InvalidDescriptor(
                    "BindGroupDesc::variable_count on a layout that declares no VARIABLE_COUNT \
                     binding"
                        .to_string(),
                ));
            };
            let declared = entries
                .iter()
                .find(|slot| slot.binding == binding)
                .unwrap_or_else(|| unreachable!("the variable binding came from these entries"));
            let ceiling = declared.resolved_count(&caps.limits);
            if count > ceiling {
                return Err(HalError::InvalidDescriptor(format!(
                    "BindGroupDesc::variable_count is {count}, but binding {binding} holds at \
                     most {ceiling} descriptors"
                )));
            }
        }

        let variable_count = desc.variable_count.or_else(|| {
            variable_binding.map(|binding| variable_count_from_entries(binding, desc.entries))
        });

        // The pool holds exactly this one set, sized from the layout.
        let mut sizes: Vec<vk::DescriptorPoolSize> = Vec::with_capacity(entries.len());
        for slot in &entries {
            let count = if Some(slot.binding) == variable_binding {
                variable_count.unwrap_or(1)
            } else {
                slot.resolved_count(&caps.limits)
            };
            if count == 0 {
                continue;
            }
            let ty = conv::descriptor_type(slot.kind);
            match sizes.iter_mut().find(|size| size.ty == ty) {
                Some(size) => size.descriptor_count += count,
                None => sizes.push(vk::DescriptorPoolSize {
                    ty,
                    descriptor_count: count,
                }),
            }
        }
        if sizes.is_empty() {
            // A layout with only zero-length variable arrays. Vulkan forbids an
            // empty pool, and one descriptor is cheaper than a special case.
            sizes.push(vk::DescriptorPoolSize {
                ty: vk::DescriptorType::SAMPLER,
                descriptor_count: 1,
            });
        }

        let mut pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&sizes);
        if update_after_bind {
            pool_info = pool_info.flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND);
        }
        // SAFETY: `pool_info` borrows `sizes`, which outlives the call.
        let pool = unsafe { inner.raw.create_descriptor_pool(&pool_info, None) }
            .map_err(|error| conv::hal_error("vkCreateDescriptorPool", error))?;

        let layouts = [layout_raw];
        let counts = [variable_count.unwrap_or(0)];
        let mut variable_info = vk::DescriptorSetVariableDescriptorCountAllocateInfo::default()
            .descriptor_counts(&counts);
        let mut allocate = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        if variable_count.is_some() {
            allocate = allocate.push_next(&mut variable_info);
        }
        // SAFETY: `pool` was just created with room for exactly this set, and
        // `layouts` names a live layout of this device.
        let sets = match unsafe { inner.raw.allocate_descriptor_sets(&allocate) } {
            Ok(sets) => sets,
            Err(error) => {
                // SAFETY: the pool is live, holds nothing, and is destroyed once.
                unsafe { inner.raw.destroy_descriptor_pool(pool, None) };
                return Err(conv::hal_error("vkAllocateDescriptorSets", error));
            }
        };
        let raw = sets[0];
        inner.set_object_name(raw, desc.label);

        if let Err(error) = write_descriptors(&inner, &state, raw, &entries, desc.entries) {
            // SAFETY: as above; the set belongs to this pool and dies with it.
            unsafe { inner.raw.destroy_descriptor_pool(pool, None) };
            return Err(error);
        }

        Ok(inner.stamp(state.bind_groups.insert(BindGroupEntryRecord {
            owner: inner.id,
            raw,
            pool,
            layout: desc.layout,
            update_after_bind,
        })))
    }

    pub(crate) fn update_bind_group_impl(
        &self,
        group: BindGroupHandle,
        updates: &[BindGroupEntry],
    ) -> Result<(), HalError> {
        let inner = Arc::clone(self.inner());
        let state = inner.state();
        let record = lookup(&state.bind_groups, "bind group", group, &inner)?;
        if !record.update_after_bind {
            // The seam names this exact error: without UPDATE_AFTER_BIND,
            // writing a set that a pending command buffer might reference is
            // undefined behaviour, so refusing is the only safe answer.
            return Err(HalError::Unsupported {
                backend: crcbl_hal::BackendKind::Vulkan,
                what: "update_bind_group on a layout without UPDATE_AFTER_BIND",
            });
        }
        let raw = record.raw;
        let layout = lookup(
            &state.bind_group_layouts,
            "bind group layout",
            record.layout,
            &inner,
        )?;
        let entries = layout.entries.clone();
        write_descriptors(&inner, &state, raw, &entries, updates)
    }

    pub(crate) fn create_pipeline_layout_impl(
        &self,
        desc: &PipelineLayoutDesc<'_>,
    ) -> Result<PipelineLayoutHandle, HalError> {
        let inner = Arc::clone(self.inner());
        let caps = inner.caps;
        let mut state = inner.state();

        if desc.bind_group_layouts.len() > caps.limits.max_bind_groups as usize {
            return Err(HalError::InvalidDescriptor(format!(
                "{} bind groups exceeds max_bind_groups {}",
                desc.bind_group_layouts.len(),
                caps.limits.max_bind_groups
            )));
        }
        let mut set_layouts = Vec::with_capacity(desc.bind_group_layouts.len());
        for handle in desc.bind_group_layouts {
            let entry = lookup(
                &state.bind_group_layouts,
                "bind group layout",
                *handle,
                &inner,
            )?;
            set_layouts.push(entry.raw);
        }

        let mut ranges = Vec::new();
        let (push_constant_stages, push_constant_size) = match desc.push_constants {
            Some(range) => {
                if !caps.features.contains(Features::PUSH_CONSTANTS) {
                    // Loudly at layout creation, never by dropping the writes
                    // later — the seam calls this out as the Tier B trap it
                    // exists to make visible.
                    return Err(HalError::Unsupported {
                        backend: crcbl_hal::BackendKind::Vulkan,
                        what: "push constants on a device without PUSH_CONSTANTS",
                    });
                }
                let end = range.offset.saturating_add(range.size);
                if end > caps.limits.max_push_constant_size {
                    return Err(HalError::InvalidDescriptor(format!(
                        "push constant range ends at {end} but max_push_constant_size is {}",
                        caps.limits.max_push_constant_size
                    )));
                }
                if range.size == 0 || range.size % 4 != 0 || range.offset % 4 != 0 {
                    return Err(HalError::InvalidDescriptor(format!(
                        "push constant range {}..{end} must be a non-empty multiple of 4 bytes \
                         at a 4-byte offset",
                        range.offset
                    )));
                }
                range
                    .stages
                    .check_supported(caps.features, crcbl_hal::BackendKind::Vulkan)?;
                let stages = conv::shader_stages(range.stages);
                if stages.is_empty() {
                    return Err(HalError::InvalidDescriptor(
                        "a push constant range must name at least one stage".to_string(),
                    ));
                }
                ranges.push(vk::PushConstantRange {
                    stage_flags: stages,
                    offset: range.offset,
                    size: range.size,
                });
                (stages, range.size)
            }
            None => (vk::ShaderStageFlags::empty(), 0),
        };

        let info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&ranges);
        // SAFETY: `info` borrows two locals that outlive the call, and every
        // set layout was resolved against this device's table above.
        let raw = unsafe { inner.raw.create_pipeline_layout(&info, None) }
            .map_err(|error| conv::hal_error("vkCreatePipelineLayout", error))?;
        inner.set_object_name(raw, desc.label);

        Ok(
            inner.stamp(state.pipeline_layouts.insert(PipelineLayoutEntry {
                owner: inner.id,
                raw,
                push_constant_stages,
                push_constant_size,
            })),
        )
    }

    pub(crate) fn create_graphics_pipeline_impl(
        &self,
        desc: &GraphicsPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let inner = Arc::clone(self.inner());
        let mut state = inner.state();

        let samples = validate_rendering_state(
            &inner.caps,
            desc.primitive,
            desc.depth_stencil,
            desc.multisample,
            desc.color_targets,
        )?;

        let layout = lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            desc.layout,
            &inner,
        )?;
        let layout_raw = layout.raw;

        // The entry-point names have to outlive `vkCreateGraphicsPipelines`, so
        // they are owned locals rather than temporaries in the builder chain.
        let (vertex_name, vertex_module) =
            entry_name(&state, &inner, desc.vertex, ShaderStages::VERTEX)?;
        let fragment_name = match desc.fragment {
            Some(entry) => Some(entry_name(&state, &inner, entry, ShaderStages::FRAGMENT)?),
            None => None,
        };

        let mut stages = vec![
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex_module)
                .name(&vertex_name),
        ];
        if let Some((name, module)) = fragment_name.as_ref() {
            stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(*module)
                    .name(name),
            );
        }

        create_rendering_pipeline(
            &inner,
            &mut state,
            &RenderingState {
                label: desc.label,
                layout: layout_raw,
                stages: &stages,
                topology: Some(desc.primitive.topology),
                primitive: desc.primitive,
                depth_stencil: desc.depth_stencil,
                multisample: desc.multisample,
                samples,
                color_targets: desc.color_targets,
            },
        )
    }

    /// Builds a mesh pipeline — `VK_EXT_mesh_shader`'s stages in an otherwise
    /// ordinary `vkCreateGraphicsPipelines` call.
    ///
    /// Everything after the geometry stages is shared with
    /// [`create_graphics_pipeline_impl`](Self::create_graphics_pipeline_impl)
    /// through [`create_rendering_pipeline`], because on Vulkan a mesh pipeline
    /// genuinely *is* a graphics pipeline: same create function, same handle,
    /// same `VK_PIPELINE_BIND_POINT_GRAPHICS`. What differs is two stage
    /// entries and the absence of `pVertexInputState` and
    /// `pInputAssemblyState`, which the spec says are ignored for a pipeline
    /// carrying a mesh stage and which this therefore does not build at all.
    pub(crate) fn create_mesh_pipeline_impl(
        &self,
        desc: &crcbl_hal::MeshPipelineDesc<'_>,
    ) -> Result<GraphicsPipelineHandle, HalError> {
        let inner = Arc::clone(self.inner());
        let caps = inner.caps;

        // First, and before any handle is resolved: the caller needs to hear
        // which capability is missing, not which handle was stale. The
        // extension is only *enabled* when the device reported the flag (see
        // `device.rs`), so this is also what stops a call through an
        // unloaded `vkCmdDrawMeshTasksEXT` later.
        if !caps.features.contains(Features::MESH_SHADER) {
            return Err(HalError::Unsupported {
                backend: crcbl_hal::BackendKind::Vulkan,
                what: "mesh shading on a device without MESH_SHADER",
            });
        }
        if desc.task.is_some() && !caps.features.contains(Features::TASK_SHADER) {
            return Err(HalError::Unsupported {
                backend: crcbl_hal::BackendKind::Vulkan,
                what: "a task stage on a device without TASK_SHADER",
            });
        }

        let mut state = inner.state();
        let samples = validate_rendering_state(
            &caps,
            desc.primitive,
            desc.depth_stencil,
            desc.multisample,
            desc.color_targets,
        )?;

        let layout = lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            desc.layout,
            &inner,
        )?;
        let layout_raw = layout.raw;

        // Owned for the same reason the raster path's are: the builder stores
        // the pointer and `vkCreateGraphicsPipelines` reads it after this
        // statement.
        let task_name = match desc.task {
            Some(entry) => Some(entry_name(&state, &inner, entry, ShaderStages::TASK)?),
            None => None,
        };
        let (mesh_name, mesh_module) = entry_name(&state, &inner, desc.mesh, ShaderStages::MESH)?;
        // The same guard the compute path applies, for the same reason and on
        // the two stages a mesh pipeline has: the seam's workgroup sizes exist
        // because Metal takes them at `drawMeshThreadgroups:` and has no
        // declaration to compare them with, and this backend is compiling the
        // module that declares one. Both sizes are checked here rather than
        // only the mesh one, because a wrong task size launches the wrong
        // number of amplification threads and nothing on Vulkan would notice.
        desc.check_workgroup_sizes()?;
        for (entry, declared) in desc
            .task
            .map(|task| (task, desc.task_workgroup_size))
            .into_iter()
            .chain([(desc.mesh, desc.mesh_workgroup_size)])
        {
            let module = lookup(&state.shader_modules, "shader module", entry.module, &inner)?;
            spirv::require_workgroup_size(&module.words, entry.entry_point, declared)
                .map_err(HalError::ShaderCompilation)?;
        }
        let fragment_name = match desc.fragment {
            Some(entry) => Some(entry_name(&state, &inner, entry, ShaderStages::FRAGMENT)?),
            None => None,
        };

        let mut stages = Vec::with_capacity(3);
        if let Some((name, module)) = task_name.as_ref() {
            stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::TASK_EXT)
                    .module(*module)
                    .name(name),
            );
        }
        stages.push(
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::MESH_EXT)
                .module(mesh_module)
                .name(&mesh_name),
        );
        if let Some((name, module)) = fragment_name.as_ref() {
            stages.push(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(*module)
                    .name(name),
            );
        }

        create_rendering_pipeline(
            &inner,
            &mut state,
            &RenderingState {
                label: desc.label,
                layout: layout_raw,
                stages: &stages,
                // No input primitives to assemble, so no input-assembly state
                // and no vertex-input state. The mesh shader's
                // `[outputtopology(…)]` is the topology.
                topology: None,
                primitive: desc.primitive,
                depth_stencil: desc.depth_stencil,
                multisample: desc.multisample,
                samples,
                color_targets: desc.color_targets,
            },
        )
    }

    pub(crate) fn create_compute_pipeline_impl(
        &self,
        desc: &ComputePipelineDesc<'_>,
    ) -> Result<ComputePipelineHandle, HalError> {
        let inner = Arc::clone(self.inner());
        let mut state = inner.state();

        let layout = lookup(
            &state.pipeline_layouts,
            "pipeline layout",
            desc.layout,
            &inner,
        )?;
        let layout_raw = layout.raw;

        let (name, module) = entry_name(&state, &inner, desc.compute, ShaderStages::COMPUTE)?;

        // The seam's workgroup size exists for Metal, which takes it at the
        // dispatch call and has no declaration to compare it with. This backend
        // is compiling the module that declares one, so it is where a
        // disagreement can be caught at all. Both halves: a size no device could
        // launch, and a size that is not the one the module asks for.
        desc.check_workgroup_size(&inner.caps.limits)?;
        let entry = lookup(
            &state.shader_modules,
            "shader module",
            desc.compute.module,
            &inner,
        )?;
        spirv::require_workgroup_size(&entry.words, desc.compute.entry_point, desc.workgroup_size)
            .map_err(HalError::ShaderCompilation)?;

        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(module)
            .name(&name);
        let info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(layout_raw);

        // SAFETY: as `create_graphics_pipeline_impl`.
        let created = unsafe {
            inner
                .raw
                .create_compute_pipelines(vk::PipelineCache::null(), &[info], None)
        };
        let raw = match created {
            Ok(pipelines) => pipelines[0],
            Err((pipelines, error)) => {
                for pipeline in pipelines {
                    if pipeline != vk::Pipeline::null() {
                        // SAFETY: as above.
                        unsafe { inner.raw.destroy_pipeline(pipeline, None) };
                    }
                }
                return Err(HalError::PipelineCreation(format!(
                    "vkCreateComputePipelines: {error:?}"
                )));
            }
        };
        inner.set_object_name(raw, desc.label);

        Ok(inner.stamp(state.pipelines.insert(PipelineEntry {
            owner: inner.id,
            raw,
        })))
    }

    pub(crate) fn create_sampler_impl(
        &self,
        desc: &SamplerDesc<'_>,
    ) -> Result<SamplerHandle, HalError> {
        let caps = self.inner().caps;
        if desc.anisotropy > caps.limits.max_sampler_anisotropy {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} exceeds max_sampler_anisotropy {}",
                desc.anisotropy, caps.limits.max_sampler_anisotropy
            )));
        }
        if desc.anisotropy < 1.0 {
            return Err(HalError::InvalidDescriptor(format!(
                "anisotropy {} is below 1.0, which is the value that disables it",
                desc.anisotropy
            )));
        }
        let anisotropy_enabled = desc.anisotropy > 1.0;
        if anisotropy_enabled && !caps.features.contains(Features::SAMPLER_ANISOTROPY) {
            return Err(HalError::Unsupported {
                backend: crcbl_hal::BackendKind::Vulkan,
                what: "anisotropic sampling on a device without SAMPLER_ANISOTROPY",
            });
        }

        let info = vk::SamplerCreateInfo::default()
            .mag_filter(conv::filter(desc.mag_filter))
            .min_filter(conv::filter(desc.min_filter))
            .mipmap_mode(conv::mipmap_mode(desc.mip_filter))
            .address_mode_u(conv::address_mode(desc.address_mode[0]))
            .address_mode_v(conv::address_mode(desc.address_mode[1]))
            .address_mode_w(conv::address_mode(desc.address_mode[2]))
            .min_lod(desc.lod_min)
            // `f32::MAX` is the seam's "no ceiling"; Vulkan spells that
            // `VK_LOD_CLAMP_NONE`, which is the same value, but going through
            // the constant is what makes the intent survive a reader.
            .max_lod(if desc.lod_max >= vk::LOD_CLAMP_NONE {
                vk::LOD_CLAMP_NONE
            } else {
                desc.lod_max
            })
            .anisotropy_enable(anisotropy_enabled)
            .max_anisotropy(desc.anisotropy)
            .compare_enable(desc.compare.is_some())
            .compare_op(desc.compare.map_or(vk::CompareOp::NEVER, conv::compare_op))
            // Transparent black, which is what the seam's `ClampToBorder`
            // promises and what a shadow atlas needs; opaque black would put a
            // black frame around every clamped sample.
            .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK);

        // SAFETY: `info` is fully populated with no chained structs.
        let raw = unsafe { self.inner().raw.create_sampler(&info, None) }
            .map_err(|error| conv::hal_error("vkCreateSampler", error))?;
        self.inner().set_object_name(raw, desc.label);

        Ok(self
            .inner()
            .stamp(self.inner().state().samplers.insert(SamplerEntry {
                owner: self.inner().id,
                raw,
            })))
    }
}

// --- helpers ---------------------------------------------------------------

/// The half of a graphics pipeline that a mesh pipeline shares with a raster
/// one: everything after the geometry stages.
///
/// A struct rather than nine parameters, and borrowed rather than owned,
/// because every field is already a local of the caller that outlives the
/// `vkCreateGraphicsPipelines` call.
struct RenderingState<'a> {
    label: Option<&'a str>,
    layout: vk::PipelineLayout,
    stages: &'a [vk::PipelineShaderStageCreateInfo<'a>],
    /// `Some` for a raster pipeline; `None` for a mesh one, which has no input
    /// primitives to assemble and therefore neither an input-assembly nor a
    /// vertex-input state.
    topology: Option<crcbl_hal::PrimitiveTopology>,
    primitive: crcbl_hal::PrimitiveState,
    depth_stencil: Option<crcbl_hal::DepthStencilState>,
    multisample: crcbl_hal::MultisampleState,
    samples: vk::SampleCountFlags,
    color_targets: &'a [crcbl_hal::ColorTargetState],
}

/// The state checks a raster and a mesh pipeline share, in the order the raster
/// path has always made them, returning the resolved sample count.
///
/// # Errors
///
/// [`HalError::InvalidDescriptor`] for a value out of range, or
/// [`HalError::Unsupported`] for state the device lacks the feature for.
fn validate_rendering_state(
    caps: &DeviceCaps,
    primitive: crcbl_hal::PrimitiveState,
    depth_stencil: Option<crcbl_hal::DepthStencilState>,
    multisample: crcbl_hal::MultisampleState,
    color_targets: &[crcbl_hal::ColorTargetState],
) -> Result<vk::SampleCountFlags, HalError> {
    if color_targets.len() > caps.limits.max_color_attachments as usize {
        return Err(HalError::InvalidDescriptor(format!(
            "{} colour targets exceeds max_color_attachments {}",
            color_targets.len(),
            caps.limits.max_color_attachments
        )));
    }
    if primitive.polygon_mode == crcbl_hal::PolygonMode::Line
        && !caps.features.contains(Features::POLYGON_MODE_LINE)
    {
        return Err(HalError::Unsupported {
            backend: crcbl_hal::BackendKind::Vulkan,
            what: "PolygonMode::Line on a device without POLYGON_MODE_LINE",
        });
    }
    if primitive.depth_clamp && !caps.features.contains(Features::DEPTH_CLAMP) {
        return Err(HalError::Unsupported {
            backend: crcbl_hal::BackendKind::Vulkan,
            what: "depth clamping on a device without DEPTH_CLAMP",
        });
    }
    let Some(samples) = conv::sample_count(multisample.samples) else {
        return Err(HalError::InvalidDescriptor(format!(
            "MultisampleState::samples is {}, which is not a power of two in 1..=64",
            multisample.samples
        )));
    };
    if let Some(depth) = depth_stencil
        && !depth.format.is_depth_stencil()
    {
        return Err(HalError::InvalidDescriptor(format!(
            "DepthStencilState::format is {:?}, which is not a depth/stencil format",
            depth.format
        )));
    }
    Ok(samples)
}

/// Assembles and creates the pipeline once its stages are resolved.
///
/// Shared by the raster and the mesh path, because on Vulkan they differ only
/// in which stages they carry: both go through `vkCreateGraphicsPipelines`,
/// both land in the same pool, and both are bound at
/// `VK_PIPELINE_BIND_POINT_GRAPHICS`.
///
/// # Errors
///
/// [`HalError::PipelineCreation`] with the driver's result.
fn create_rendering_pipeline(
    inner: &Arc<DeviceInner>,
    state: &mut DeviceState,
    desc: &RenderingState<'_>,
) -> Result<GraphicsPipelineHandle, HalError> {
    // Empty, and it stays empty — and for a mesh pipeline it is not even
    // attached. See the module docs.
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let assembly = desc.topology.map(|topology| {
        vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(conv::topology(topology))
            .primitive_restart_enable(false)
    });
    // Counts without pointers: both are dynamic state, set per pass by the
    // encoder, because a pipeline that baked a viewport would need
    // recreating on every window resize.
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let bias = desc.depth_stencil.map(|depth| depth.bias);
    let bias_enabled = bias.is_some_and(|bias| bias.constant != 0.0 || bias.slope_scale != 0.0);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(desc.primitive.depth_clamp)
        .rasterizer_discard_enable(false)
        .polygon_mode(conv::polygon_mode(desc.primitive.polygon_mode))
        .cull_mode(conv::cull_mode(desc.primitive.cull_mode))
        .front_face(conv::front_face(desc.primitive.front_face))
        .depth_bias_enable(bias_enabled)
        .depth_bias_constant_factor(bias.map_or(0.0, |bias| bias.constant))
        .depth_bias_clamp(bias.map_or(0.0, |bias| bias.clamp))
        .depth_bias_slope_factor(bias.map_or(0.0, |bias| bias.slope_scale))
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(desc.samples)
        .sample_shading_enable(false)
        // Every sample covered, said the way the spec spells it: an empty slice
        // is a null `pSampleMask`, which Vulkan defines as all bits set. That is
        // what the seam guarantees — `MultisampleState` carries no mask, because
        // `MTLRenderPipelineDescriptor` has no member to put one in — and saying
        // it with a null beats an array of ones, whose length would have to be
        // `ceil(rasterization_samples / 32)` and is a second thing to get wrong.
        .sample_mask(&[])
        .alpha_to_coverage_enable(desc.multisample.alpha_to_coverage);

    let depth_stencil = desc.depth_stencil.map(|depth| {
        let stencil = depth.stencil.unwrap_or_default();
        vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            // Reversed-Z reaches the driver untouched: the seam's default
            // is `Greater`, `conv::compare_op` is a pure rename, and the
            // matching clear value is `depth::CLEAR` = 0.0.
            .depth_compare_op(conv::compare_op(depth.depth_compare))
            .depth_write_enable(depth.depth_write)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(depth.stencil.is_some())
            .front(conv::stencil_face(stencil.front, stencil))
            .back(conv::stencil_face(stencil.back, stencil))
    });

    let attachments: Vec<vk::PipelineColorBlendAttachmentState> = desc
        .color_targets
        .iter()
        .map(|target| {
            let mut state = vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(conv::color_writes(target.write_mask))
                .blend_enable(target.blend.is_some());
            if let Some(blend) = target.blend {
                state = state
                    .src_color_blend_factor(conv::blend_factor(blend.color_src))
                    .dst_color_blend_factor(conv::blend_factor(blend.color_dst))
                    .color_blend_op(conv::blend_op(blend.color_op))
                    .src_alpha_blend_factor(conv::blend_factor(blend.alpha_src))
                    .dst_alpha_blend_factor(conv::blend_factor(blend.alpha_dst))
                    .alpha_blend_op(conv::blend_op(blend.alpha_op));
            }
            state
        })
        .collect();
    let blend = vk::PipelineColorBlendStateCreateInfo::default()
        .logic_op_enable(false)
        .attachments(&attachments);

    // `STENCIL_REFERENCE` because the reference is pass state on the seam —
    // `set_stencil_reference` is the only channel, `StencilState` has no field
    // to bake, and a pipeline that baked one would make that call a no-op. It
    // is declared on *every* pipeline, stencil or not, so the encoder never has
    // to know which pipelines would honour the call. `begin_render_pass` sets
    // `stencil::INITIAL_REFERENCE` in exchange: with this dynamic state
    // declared, the value is undefined until something sets it.
    let dynamic_states = [
        vk::DynamicState::VIEWPORT,
        vk::DynamicState::SCISSOR,
        vk::DynamicState::STENCIL_REFERENCE,
    ];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    let color_formats: Vec<vk::Format> = desc
        .color_targets
        .iter()
        .map(|target| conv::format(target.format))
        .collect();
    let depth_format = desc
        .depth_stencil
        .map_or(vk::Format::UNDEFINED, |depth| conv::format(depth.format));
    let mut rendering = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(&color_formats)
        .depth_attachment_format(depth_format);
    if desc
        .depth_stencil
        .is_some_and(|depth| depth.format.has_stencil())
    {
        rendering = rendering.stencil_attachment_format(depth_format);
    }

    let mut info = vk::GraphicsPipelineCreateInfo::default()
        .stages(desc.stages)
        .viewport_state(&viewport)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(desc.layout)
        .push_next(&mut rendering);
    if let Some(assembly) = assembly.as_ref() {
        info = info
            .vertex_input_state(&vertex_input)
            .input_assembly_state(assembly);
    }
    if let Some(depth_stencil) = depth_stencil.as_ref() {
        info = info.depth_stencil_state(depth_stencil);
    }

    // SAFETY: every pointer in `info` borrows a local that outlives the
    // call; the modules and the layout were resolved against this device's
    // tables; and the entry points were checked to exist in the modules.
    let created = unsafe {
        inner
            .raw
            .create_graphics_pipelines(vk::PipelineCache::null(), &[info], None)
    };
    let raw = match created {
        Ok(pipelines) => pipelines[0],
        // `create_graphics_pipelines` returns the partial results alongside
        // the error, and any pipeline it did manage to create is ours to
        // free — dropping the `Vec` would leak them.
        Err((pipelines, error)) => {
            for pipeline in pipelines {
                if pipeline != vk::Pipeline::null() {
                    // SAFETY: created by this device moments ago, never
                    // used, destroyed once.
                    unsafe { inner.raw.destroy_pipeline(pipeline, None) };
                }
            }
            return Err(HalError::PipelineCreation(format!(
                "vkCreateGraphicsPipelines: {error:?}"
            )));
        }
    };
    inner.set_object_name(raw, desc.label);

    Ok(inner.stamp(state.pipelines.insert(PipelineEntry {
        owner: inner.id,
        raw,
    })))
}

/// Resolves a stage's entry point, checking it exists in the module first.
///
/// Returns the `VkShaderModule` alongside the name because every caller needs
/// both, and looking the same handle up twice was the shape that produced it.
fn entry_name(
    state: &DeviceState,
    inner: &DeviceInner,
    entry: ShaderEntry<'_>,
    stage: ShaderStages,
) -> Result<(std::ffi::CString, vk::ShaderModule), HalError> {
    let module = lookup(&state.shader_modules, "shader module", entry.module, inner)?;
    spirv::require_entry_point(&module.words, entry.entry_point, stage)
        .map_err(HalError::ShaderCompilation)?;
    let name = std::ffi::CString::new(entry.entry_point).map_err(|_| {
        HalError::ShaderCompilation(format!(
            "entry point {:?} contains a NUL byte",
            entry.entry_point
        ))
    })?;
    Ok((name, module.raw))
}

/// Whether a resource can fill a binding of that kind.
///
/// Vulkan reports the mismatch, but as a validation message naming a descriptor
/// type rather than the caller's binding number — and only in a debug build
/// with the layer loaded.
fn check_resource_kind(
    kind: BindingKind,
    resource: &BindingResource,
    binding: u32,
) -> Result<(), HalError> {
    let ok = matches!(
        (kind, resource),
        (
            BindingKind::UniformBuffer { .. } | BindingKind::StorageBuffer { .. },
            BindingResource::Buffer { .. }
        ) | (
            BindingKind::SampledImage { .. } | BindingKind::StorageImage { .. },
            BindingResource::ImageView(_)
        ) | (BindingKind::Sampler { .. }, BindingResource::Sampler(_))
    );
    if ok {
        return Ok(());
    }
    Err(HalError::InvalidDescriptor(format!(
        "binding {binding} is declared as {kind:?}, which cannot hold {resource:?}"
    )))
}

/// Writes descriptors into an already-allocated set.
///
/// The info structs are collected **before** any write references them: `ash`'s
/// builders store pointers, so building a write against a `Vec` that is still
/// growing would leave every earlier write pointing into a freed allocation.
fn write_descriptors(
    inner: &DeviceInner,
    state: &DeviceState,
    set: vk::DescriptorSet,
    declared: &[BindGroupLayoutEntry],
    entries: &[BindGroupEntry],
) -> Result<(), HalError> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut buffers: Vec<vk::DescriptorBufferInfo> = Vec::with_capacity(entries.len());
    let mut images: Vec<vk::DescriptorImageInfo> = Vec::with_capacity(entries.len());
    // (index into buffers/images, is_buffer, binding, array_index, type)
    let mut plan: Vec<(usize, bool, u32, u32, vk::DescriptorType)> =
        Vec::with_capacity(entries.len());

    for entry in entries {
        let Some(slot) = declared.iter().find(|slot| slot.binding == entry.binding) else {
            return Err(HalError::InvalidDescriptor(format!(
                "bind group entry names binding {}, which the layout does not declare",
                entry.binding
            )));
        };
        check_resource_kind(slot.kind, &entry.resource, entry.binding)?;
        let ty = conv::descriptor_type(slot.kind);

        match entry.resource {
            BindingResource::Buffer {
                buffer,
                offset,
                size,
            } => {
                // `Limits::min_*_buffer_offset_alignment` was populated by
                // `adapter.rs` and read by nothing, so a misaligned binding
                // offset became `VUID-VkWriteDescriptorSet-descriptorType-00327`
                // in the driver instead of an `InvalidDescriptor` naming the
                // binding.
                let alignment = match slot.kind {
                    BindingKind::UniformBuffer { .. } => {
                        inner.caps.limits.min_uniform_buffer_offset_alignment
                    }
                    BindingKind::StorageBuffer { .. } => {
                        inner.caps.limits.min_storage_buffer_offset_alignment
                    }
                    _ => 1,
                };
                if alignment > 1 && !offset.is_multiple_of(alignment) {
                    return Err(HalError::InvalidDescriptor(format!(
                        "binding {}'s buffer offset {offset} is not a multiple of this device's \
                         {alignment}-byte alignment for {:?}",
                        entry.binding, slot.kind
                    )));
                }
                // Same story for the two range limits: a binding longer than
                // the slot allows is
                // `VUID-VkWriteDescriptorSet-descriptorType-00332` (uniform) or
                // `-00333` (storage), which validation layers report and a
                // release driver does not. `WHOLE_BUFFER` is resolved into what
                // it actually covers first — it is the commoner spelling, and
                // it carries no number to compare.
                let (raw, length) = inner.buffer_raw_and_size(state, buffer)?;
                let ceiling = match slot.kind {
                    BindingKind::UniformBuffer { .. } => {
                        Some(inner.caps.limits.max_uniform_buffer_range)
                    }
                    BindingKind::StorageBuffer { .. } => {
                        Some(inner.caps.limits.max_storage_buffer_range)
                    }
                    _ => None,
                };
                let bound = if size == BindingResource::WHOLE_BUFFER {
                    length.saturating_sub(offset)
                } else {
                    size
                };
                if let Some(ceiling) = ceiling
                    && bound > ceiling
                {
                    return Err(HalError::InvalidDescriptor(format!(
                        "binding {} is given {bound} bytes of a buffer, over this device's \
                         {ceiling}-byte limit for {:?}",
                        entry.binding, slot.kind
                    )));
                }
                plan.push((buffers.len(), true, entry.binding, entry.array_index, ty));
                buffers.push(vk::DescriptorBufferInfo {
                    buffer: raw,
                    offset,
                    range: if size == BindingResource::WHOLE_BUFFER {
                        vk::WHOLE_SIZE
                    } else {
                        size
                    },
                });
            }
            BindingResource::ImageView(view) => {
                let (raw, _) = inner.view_raw(state, view)?;
                // The layout a sampled image is read in. The graph owns the
                // transition — the seam says so explicitly — so this states the
                // layout the descriptor will be *used* in, which for a sampled
                // image is `SHADER_READ_ONLY_OPTIMAL` and for a storage image
                // is `GENERAL`, matching `conv::state_masks`.
                let layout = match slot.kind {
                    BindingKind::StorageImage { .. } => vk::ImageLayout::GENERAL,
                    _ => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                };
                plan.push((images.len(), false, entry.binding, entry.array_index, ty));
                images.push(vk::DescriptorImageInfo {
                    sampler: vk::Sampler::null(),
                    image_view: raw,
                    image_layout: layout,
                });
            }
            BindingResource::Sampler(sampler) => {
                let raw = lookup(&state.samplers, "sampler", sampler, inner)?.raw;
                plan.push((images.len(), false, entry.binding, entry.array_index, ty));
                images.push(vk::DescriptorImageInfo {
                    sampler: raw,
                    image_view: vk::ImageView::null(),
                    image_layout: vk::ImageLayout::UNDEFINED,
                });
            }
        }
    }

    // Nothing is pushed into either `Vec` from here on, so the borrows below
    // are stable for the duration of the call.
    let writes: Vec<vk::WriteDescriptorSet<'_>> = plan
        .iter()
        .map(|(index, is_buffer, binding, array_index, ty)| {
            let write = vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(*binding)
                .dst_array_element(*array_index)
                .descriptor_type(*ty);
            if *is_buffer {
                write.buffer_info(std::slice::from_ref(&buffers[*index]))
            } else {
                write.image_info(std::slice::from_ref(&images[*index]))
            }
        })
        .collect();

    // SAFETY: `set` belongs to this device, every write names a binding the
    // layout declares with a matching descriptor type, and each write's
    // `*_info` borrows a `Vec` element that outlives the call.
    unsafe { inner.raw.update_descriptor_sets(&writes, &[]) };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crcbl_hal::{ImageViewType, SampleType};

    // The layout rules themselves are tested where they now live —
    // `BindGroupLayoutDesc::check_entries` in `crcbl-hal`. That this backend
    // *calls* it is what `vk_e2e::pipeline`'s bindless probe asserts, on a real
    // device, because that is the only place the two can be told apart.

    /// The inference documented on [`variable_count_from_entries`], pinned so
    /// the gap it works around stays visible.
    #[test]
    fn the_variable_count_is_inferred_from_the_highest_slot_written() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let buffer: crcbl_hal::BufferHandle = pool.insert(0).cast();
        let at = |binding, array_index| BindGroupEntry {
            binding,
            array_index,
            resource: BindingResource::whole_buffer(buffer),
        };

        assert_eq!(
            variable_count_from_entries(1, &[at(1, 0), at(1, 3), at(1, 1)]),
            4,
            "one past the highest index written"
        );
        // Entries for another binding must not inflate it.
        assert_eq!(variable_count_from_entries(1, &[at(0, 99), at(1, 0)]), 1);
        // Nothing written is still a legal, allocatable set.
        assert_eq!(variable_count_from_entries(1, &[]), 1);
    }

    /// A resource of the wrong shape must be named at the seam's own binding
    /// number, not left to a validation message about descriptor types.
    #[test]
    fn a_mismatched_resource_names_the_binding_it_could_not_fill() {
        let mut pool: crcbl_core::Pool<u8> = crcbl_core::Pool::new();
        let buffer: crcbl_hal::BufferHandle = pool.insert(0).cast();
        let sampler: SamplerHandle = pool.insert(0).cast();

        check_resource_kind(
            BindingKind::StorageBuffer {
                read_only: true,
                dynamic: false,
            },
            &BindingResource::whole_buffer(buffer),
            7,
        )
        .expect("a buffer fills a buffer binding");

        let error = check_resource_kind(
            BindingKind::SampledImage {
                view_type: ImageViewType::D2,
                sample_type: SampleType::Float,
            },
            &BindingResource::whole_buffer(buffer),
            7,
        )
        .expect_err("a buffer cannot fill an image binding");
        assert!(error.to_string().contains('7'), "{error}");

        check_resource_kind(
            BindingKind::Sampler { comparison: false },
            &BindingResource::Sampler(sampler),
            0,
        )
        .expect("a sampler fills a sampler binding");
        assert!(
            check_resource_kind(
                BindingKind::SampledImage {
                    view_type: ImageViewType::D2,
                    sample_type: SampleType::Float,
                },
                &BindingResource::Sampler(sampler),
                0
            )
            .is_err(),
            "separate samplers: a sampler is not an image"
        );
    }
}
