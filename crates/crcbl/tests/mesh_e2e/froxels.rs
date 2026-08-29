//! The froxel column itself, copied back and checked cell by cell against
//! `crcbl_shaders::volumetric` on the host.
//!
//! [`hdr`](crate::hdr) measures what the medium does to a *picture*: a
//! transmittance recovered per texel, a lobe that brightens the frame the sun
//! is ahead of, a shaft that has an edge in it. Every one of those reads the
//! composited frame, and there are two passes between the medium and that
//! frame — `scatterMain` shades each slab on its own, `integrateMain` turns the
//! column into the exclusive prefix in front of each slab.
//!
//! **A frame cannot tell those two apart.** The environment term's
//! single-scattering albedo is one, so a slab scatters exactly what it
//! extinguishes; a scatter that is wrong by a factor and a scan that is wrong
//! by its reciprocal compose back to the closed form the frame-level checks
//! compare against. This module is what separates them: it reads the column out
//! of the buffer, rebuilds every froxel's own slab from the parameter block the
//! shaders were handed, scans it here, and compares — so a wrong slab and a
//! wrong scan fail at different froxels and neither can hide behind the other.
//!
//! **What that is worth was measured rather than argued.** Two mistakes were
//! put into `integrateMain` in turn — the scan made inclusive, and its last
//! slice dropped — and each was rebuilt and run against this suite. This test
//! failed on both, at froxel 0 and at froxel 284. The frame-level checks failed
//! on both too, but on a *different* one each time: the inclusive scan reddened
//! the closed-form equality, and the dropped slice reddened the occluder's
//! shaft. Neither says which of the three passes moved, and a pair of
//! compensating mistakes is a frame they would not fail at all.
//!
//! # What is modelled and what is read
//!
//! Everything the scatter pass computes is rebuilt here **except the shadow
//! lookup**, which is read out of the lighting buffer that pass writes beside
//! the column. That is not a gap: the atlas walk is `mesh.slang`'s, letter for
//! letter, and `crcbl_shaders::volumetric`'s `both_shaders_spell_the_same_atlas_walk`
//! is what holds the two copies together. What this module needs from it is a
//! number per froxel, and the buffer is where the composite reads that number
//! too — see
//! [`LIGHTING_STRIDE`](crcbl::shaders::volumetric::LIGHTING_STRIDE).
//!
//! The other three lanes of that buffer — what the froxel's punctual lights
//! scatter into it — are **not** read back into the model but rebuilt from the
//! fixture's own two lights and compared, in
//! [`the_glow_in_the_buffer_is_the_froxel_s_list_walked_at_its_midpoint`]:
//! the cluster list is a GPU decision too, and a light that the clustering pass
//! left out of a froxel it reaches shows up here as a slab the host lit and the
//! shader did not.
//!
//! The parameter block is **read back** rather than derived a second time. A
//! model built from the host's own camera, grid and cascades would agree with a
//! wrong matrix, because the same mistake would be on both sides of the
//! comparison.

use crate::harness::{Headless, poisoned};
use crate::hdr::{SHAFT_TO_SUN, occluder_transform};
use crate::mesh_scene::{mesh_camera, place_cube, place_cube_at, render_mesh_lit};
use crcbl::hal::{
    Barriers, BufferBarrier, BufferCopy, BufferDesc, BufferUsage, CommandEncoderDesc,
    MemoryLocation, ResourceState, SubmitInfo, depth,
};
use crcbl::math::{Mat4, Vec3, Vec4};
use crcbl::render::{
    DirectionalLight, EffectOverride, EffectRequest, Fog, ForwardRenderer, FroxelBuffers, Light,
    PointLight, Projection, RenderEffects, SpotLight, TransientPool,
};
use crcbl::shaders::fog::{exp_neg, optical_depth};
use crcbl::shaders::light::{CLUSTER_FAR, CLUSTER_NEAR, KIND_SPOT, SLICE_RATIO};
use crcbl::shaders::volumetric::{
    FROXEL_STRIDE, LIGHTING_STRIDE, PARAMS_SIZE, VolumetricParams, phase,
};

/// The medium this module reads the column under.
///
/// Every field is set away from the value that would make some part of the
/// arithmetic drop out, because a froxel is compared as a number here rather
/// than as a picture:
///
/// * **The falloff is not zero**, unlike every medium in [`hdr`](crate::hdr):
///   those all take `optical_depth`'s uniform branch, and this is the only
///   fixture in the tree that puts the *height* branch under a per-froxel
///   comparison. It is short enough that the reference plane is inside the
///   scene rather than far below it, so slabs at different heights get
///   measurably different optical depths.
/// * **The three colour channels differ**, so a scatter that swizzled them or a
///   scan that carried one channel into another fails rather than landing a
///   plausible grey.
/// * **The anisotropy is a real lobe.** At `g = 0` the phase function is a
///   constant and every froxel's direction drops out of the source, which is
///   exactly the term this compares per froxel.
/// * **The sun is scattered**, so the visibility read out of the second buffer
///   is load-bearing: at `sun_scattering = 0` the whole sun term is zero and a
///   visibility of anything at all gives the same column.
/// * **And so are the lamps**, for the same reason: at `light_scattering = 0`
///   the glow lanes are zero whatever the list walk did.
const COLUMN_FOG: Fog = Fog {
    density: 0.15,
    falloff: 6.0,
    reference_height: -1.0,
    color: Vec3::new(0.7, 1.3, 2.1),
    sun_scattering: 4.0,
    light_scattering: 2.0,
    anisotropy: 0.8,
};

/// The two punctual lights the column is read under: one of each kind.
///
/// Both stand inside the frustum's near half, where the slices are short enough
/// that a radius of a unit or two spans several of them — a light out among the
/// far slices is missed at their midpoints, which `volumetric_punctual` says is
/// the rung's limit rather than this fixture's. The point light sits between
/// the eye and the cube; the spot hangs above the scene and points down, so its
/// cone closes inside its own radius and the froxels around it split into lit,
/// penumbral and dark — which is what makes `spot_cone` load-bearing here.
/// Three distinct colours, so a channel swapped in the sum fails.
fn column_lights() -> [Light; 2] {
    [
        Light::Point(PointLight {
            position: Vec3::new(0.9, 0.5, 1.1),
            radius: 1.6,
            color: Vec3::new(3.0, 1.0, 0.5),
        }),
        Light::Spot(SpotLight {
            position: Vec3::new(-0.4, 1.6, 0.4),
            radius: 2.5,
            color: Vec3::new(0.5, 2.0, 4.0),
            direction: Vec3::new(0.1, -1.0, 0.05),
            inner_angle: 0.35,
            outer_angle: 0.6,
        }),
    ]
}

/// One frame's froxel column, as the host read it.
struct Column {
    /// The block both compute passes were handed, decoded from the bytes they
    /// read rather than rebuilt.
    params: VolumetricParams,
    /// `[froxel]`: the column in front of that slab — `rgb` its accumulated
    /// radiance, `a` its transmittance — as `integrateMain` left it.
    froxels: Vec<[f32; 4]>,
    /// `[froxel]`: what that slab's lights put into it — the punctual glow in
    /// `rgb`, what fraction of the sun it sees in `a` — as `scatterMain` left
    /// it and the scan did not touch it.
    lighting: Vec<[f32; 4]>,
}

impl Column {
    /// How many froxels both passes wrote, which is what the shaders bound
    /// their indices by: the grid, or the buffer's capacity if that is smaller.
    fn count(&self) -> usize {
        let tiles = self.params.grid_x.max(1) * self.params.grid_y.max(1);
        let grid = tiles.saturating_mul(self.params.slices.max(1));
        grid.min(self.params.froxel_count) as usize
    }
}

/// Draws the shaft fixture through the froxel path and copies the column back.
///
/// The scene is [`hdr`](crate::hdr)'s shaft one — the fixture cube, the slab
/// standing between the sun and half the frustum — because that is the
/// arrangement already known to put a *varying* visibility in the buffer. A
/// column whose froxels all see the whole sun would compare just as well and
/// would say nothing about the term the second buffer carries.
///
/// `shadows` is whether the atlas is drawn into this frame. Off, the light rows
/// still name their tiles and the scatter pass still reads through them, but
/// the atlas holds its reversed-Z clear and every comparison against it comes
/// back lit — the column the glow model is checked against, and the control
/// the occlusion test measures the maps from.
fn draw_and_read_the_column(fog: Fog, shadows: bool) -> Column {
    let headless = Headless::open_for_mesh();
    let mut pool = TransientPool::new();
    let mut renderer =
        ForwardRenderer::new(headless.device.as_ref(), headless.queue, headless.format)
            .expect("the forward renderer builds");
    renderer.set_effect_request(EffectRequest {
        programmatic: EffectOverride::none()
            .force(RenderEffects::ANTIALIASING, Some(false))
            .force(RenderEffects::REFLECTIONS, Some(false))
            .force(RenderEffects::SHADOWS, Some(shadows))
            .force(RenderEffects::VOLUMETRIC_FOG, Some(true)),
        ..EffectRequest::default()
    });
    renderer.set_fog(fog);
    renderer.set_lights(&column_lights());
    place_cube(&mut renderer);
    place_cube_at(&mut renderer, occluder_transform());
    let light = DirectionalLight {
        direction: SHAFT_TO_SUN,
        ..DirectionalLight::default()
    };
    let _ = render_mesh_lit(
        &headless,
        &mut renderer,
        &mut pool,
        &mesh_camera(Projection::default()),
        &light,
        None,
    );

    let device = headless.device.as_ref();
    let buffers = renderer.froxel_buffers(renderer.frame());
    let column = read_back(&headless, buffers);

    device.wait_idle().expect("idle");
    renderer.destroy(device);
    pool.destroy(device);
    headless.finish();
    column
}

/// Copies the parameter block, the column and the lighting out of the buffers
/// the frame left them in.
///
/// One encoder for the three, and one barrier each way: all three are in
/// [`ResourceState::ShaderRead`] when a frame ends — the composite is the last
/// pass to read the column and the lighting, and the parameter block is a
/// uniform buffer nothing writes but the host.
fn read_back(headless: &Headless, buffers: FroxelBuffers) -> Column {
    let device = headless.device.as_ref();
    // The whole buffer rather than the frame's froxels: the count comes out of
    // the block this is copying, so there is nothing to size a shorter copy by
    // until it has been read.
    let froxel_bytes = u64::from(crcbl::render::FROXEL_CAPACITY) * FROXEL_STRIDE as u64;
    let lighting_bytes = u64::from(crcbl::render::FROXEL_CAPACITY) * LIGHTING_STRIDE as u64;
    let staging = |label: &str, size: u64| {
        device
            .create_buffer(&BufferDesc {
                label: Some(label),
                size,
                usage: BufferUsage::TRANSFER_DST,
                memory: MemoryLocation::HostReadback,
            })
            .expect("a readback buffer")
    };
    let copies = [
        (
            buffers.params,
            staging("froxel params", PARAMS_SIZE as u64),
            PARAMS_SIZE as u64,
        ),
        (
            buffers.froxels,
            staging("froxel column", froxel_bytes),
            froxel_bytes,
        ),
        (
            buffers.lighting,
            staging("froxel lighting", lighting_bytes),
            lighting_bytes,
        ),
    ];

    let mut encoder = device.create_command_encoder(&CommandEncoderDesc {
        label: Some("froxel column copy"),
        queue: headless.queue,
    });
    let barriers = |from: ResourceState, to: ResourceState| {
        copies.map(|(buffer, _, _)| BufferBarrier {
            buffer,
            from,
            to,
            queue_transfer: None,
        })
    };
    let out = barriers(ResourceState::ShaderRead, ResourceState::TransferSrc);
    let back = barriers(ResourceState::TransferSrc, ResourceState::ShaderRead);
    encoder.pipeline_barrier(&Barriers {
        buffers: &out,
        ..Barriers::default()
    });
    for (buffer, target, size) in copies {
        encoder.copy_buffer_to_buffer(&BufferCopy {
            src: buffer,
            src_offset: 0,
            dst: target,
            dst_offset: 0,
            size,
        });
    }
    encoder.pipeline_barrier(&Barriers {
        buffers: &back,
        ..Barriers::default()
    });
    let commands = encoder.finish().expect("recording succeeded");
    device
        .submit(headless.queue, &SubmitInfo::new(&[commands]))
        .expect("submit");

    let read = |index: usize| {
        let (_, target, size) = copies[index];
        let mut bytes = poisoned(size as usize);
        headless.readback(target, size, &mut bytes);
        device.destroy_buffer(target);
        bytes
    };
    let params = read(0);
    let froxels = read(1);
    let lighting = read(2);
    device.destroy_command_buffer(commands);

    Column {
        params: VolumetricParams::from_bytes(
            &params.try_into().expect("the block is its own size"),
        ),
        froxels: froxels
            .chunks_exact(FROXEL_STRIDE)
            .map(|cell| core::array::from_fn(|lane| float_at(cell, lane * 4)))
            .collect(),
        lighting: lighting
            .chunks_exact(LIGHTING_STRIDE)
            .map(|cell| core::array::from_fn(|lane| float_at(cell, lane * 4)))
            .collect(),
    }
}

/// The `f32` four bytes into `bytes` at `offset`, as the buffer stores it.
fn float_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four bytes are inside the cell"),
    )
}

/// The view depth slice `index` starts at, by the multiply chain
/// `volumetric.slang`'s `volumetric_slice_start` walks.
///
/// The chain rather than [`slice_near`](crcbl::shaders::light::slice_near)'s
/// `powf`, because this is one endpoint of an optical-depth integral the
/// comparison below is about: the two forms agree to within a unit in the last
/// place, and transcribing the one the shader runs is what keeps that
/// last place out of the tolerance.
fn slice_start(index: u32) -> f32 {
    (0..index).fold(CLUSTER_NEAR, |start, _| start * SLICE_RATIO)
}

/// One froxel's slab of air, as `scatterMain` cuts it: its two ends along the
/// tile's centre ray and the direction it is looked along.
struct Slab {
    from: Vec3,
    to: Vec3,
    view_direction: Vec3,
}

/// Froxel `froxel`'s slab, rebuilt from the block the shaders were handed.
fn slab(params: &VolumetricParams, froxel: usize) -> Slab {
    let grid_x = params.grid_x.max(1);
    let grid_y = params.grid_y.max(1);
    let tiles = (grid_x * grid_y) as usize;
    let slices = params.slices.max(1) as usize;
    let inverse = Mat4::from_cols_array(&params.inverse_view_proj);
    let depth_row = Vec4::from_array(params.depth_row);
    let eye = Vec4::from_array(params.eye).truncate();
    let viewport = (
        params.viewport_x.max(1) as f32,
        params.viewport_y.max(1) as f32,
    );

    let tile_x = froxel as u32 % grid_x;
    let tile_y = (froxel as u32 / grid_x) % grid_y;
    let slice = froxel / tiles;

    // The tile's centre ray, and the secant factor that goes with it: a point
    // at view depth `d` is `eye + along * d`, so a froxel at the corner of the
    // frame is longer than its depth range.
    let pixel = (
        (tile_x as f32 + 0.5) * params.tile_pixels as f32,
        (tile_y as f32 + 0.5) * params.tile_pixels as f32,
    );
    let ndc = (
        pixel.0 / viewport.0 * 2.0 - 1.0,
        1.0 - pixel.1 / viewport.1 * 2.0,
    );
    let clip = inverse * Vec4::new(ndc.0, ndc.1, depth::NEAR, 1.0);
    let near_point = clip.truncate() / clip.w;
    let near_depth = depth_row.dot(near_point.extend(1.0)).max(1e-6);
    let along = (near_point - eye) / near_depth;

    // Slice zero starts at the eye and the last one ends at the camera's far
    // plane, neither of which is where the light grid cuts — `scatterMain`
    // says why.
    let from_depth = if slice == 0 {
        0.0
    } else {
        slice_start(slice as u32)
    };
    let to_depth = if slice + 1 == slices {
        CLUSTER_FAR
    } else {
        slice_start(slice as u32 + 1)
    };
    let from = eye + along * from_depth;
    let to = eye + along * to_depth;
    let segment = to - from;
    let length_of = segment.length();
    let view_direction = if length_of > 1e-6 {
        segment / length_of
    } else {
        Vec3::Z
    };
    Slab {
        from,
        to,
        view_direction,
    }
}

/// Every froxel's own slab, and then the exclusive prefix of the column in
/// front of it — `scatterMain` and `integrateMain`, on the host.
///
/// `lighting` is the buffer the scatter pass wrote, per this module's header:
/// the sun's visibility is its `a`, and the punctual glow its `rgb`, which the
/// source adds whole exactly as the shader does.
fn modelled_column(
    params: &VolumetricParams,
    lighting: &[[f32; 4]],
    count: usize,
) -> Vec<[f32; 4]> {
    let tiles = (params.grid_x.max(1) * params.grid_y.max(1)) as usize;
    let slices = params.slices.max(1) as usize;
    let sun_direction = Vec4::from_array(params.sun_direction).truncate();
    let sun_radiance = Vec4::from_array(params.sun_radiance).truncate();
    let fog_color = Vec4::from_array(params.fog_color).truncate();
    let [density, falloff, reference, _] = params.fog_params;

    let own: Vec<[f32; 4]> = (0..count)
        .map(|froxel| {
            let Slab {
                from,
                to,
                view_direction,
            } = slab(params, froxel);
            let length_of = (to - from).length();
            let tau = optical_depth(
                density,
                falloff,
                from.y - reference,
                to.y - reference,
                length_of,
            );
            let survives = exp_neg(tau);
            let cos_theta = sun_direction.dot(view_direction);
            let [glow_r, glow_g, glow_b, visibility] = lighting[froxel];
            let source = fog_color
                + sun_radiance * phase(params.sun_direction[3], cos_theta) * visibility
                + Vec3::new(glow_r, glow_g, glow_b);
            let scattered = source * (1.0 - survives);
            [scattered.x, scattered.y, scattered.z, survives]
        })
        .collect();

    let mut prefix = vec![[0.0f32; 4]; count];
    for tile in 0..tiles {
        let mut accumulated = Vec3::ZERO;
        let mut through = 1.0f32;
        for slice in 0..slices {
            let froxel = tile + slice * tiles;
            if froxel >= count {
                break;
            }
            let [r, g, b, transmittance] = own[froxel];
            prefix[froxel] = [accumulated.x, accumulated.y, accumulated.z, through];
            accumulated += through * Vec3::new(r, g, b);
            through *= transmittance;
        }
    }
    prefix
}

/// What one froxel's punctual lights scatter into it, as `volumetric_punctual`
/// sums them: every light in `lights`, reached from the slab's midpoint, through
/// the falloff window, the cone for a spot, and the medium's phase function
/// against the slab's own view direction — then the coefficient the block
/// carries in `sun_radiance.w`.
///
/// **Every light in the scene rather than the froxel's list**, which is the
/// point: the list is `light_cluster.slang`'s decision, and a light this walk
/// reaches is one whose sphere meets the slab, so a list that left it out is a
/// froxel the host lights and the shader does not. The rows are the ones the
/// renderer uploads — [`Light::row`] — so the radius, the axis and the two
/// cosines are the numbers the shader read and not a second derivation.
fn modelled_glow(params: &VolumetricParams, lights: &[Light], froxel: usize) -> Vec3 {
    let Slab {
        from,
        to,
        view_direction,
    } = slab(params, froxel);
    let middle = (from + to) * 0.5;
    let anisotropy = params.sun_direction[3];
    let mut total = Vec3::ZERO;
    for light in lights {
        let row = light.row(None);
        let position = Vec4::from_array(row.position);
        let offset = position.truncate() - middle;
        let distance = offset.length();
        let to_light = offset / distance.max(1e-6);
        let mut reach = punctual_falloff(distance, position.w);
        if row.kind == KIND_SPOT {
            let axis = Vec4::from_array(row.direction);
            reach *= spot_cone(to_light, axis.truncate(), axis.w, row.cos_inner);
        }
        let color = Vec4::from_array(row.color).truncate();
        total += color * reach * phase(anisotropy, to_light.dot(view_direction));
    }
    total * params.sun_radiance[3]
}

/// `mesh.slang`'s `punctual_falloff`, which `volumetric.slang` copies: the
/// window that reaches exactly zero at the radius, over the inverse square
/// kept finite at the light.
fn punctual_falloff(distance: f32, radius: f32) -> f32 {
    let ratio = distance / radius.max(1e-6);
    let window = (1.0 - ratio * ratio * ratio * ratio).clamp(0.0, 1.0);
    window * window / (distance * distance + 1.0)
}

/// `mesh.slang`'s `spot_cone`, likewise — negation included, because
/// `to_light` points at the light and the cone points away from it.
fn spot_cone(to_light: Vec3, axis: Vec3, cos_outer: f32, cos_inner: f32) -> f32 {
    let cosine = (-to_light).dot(axis.normalize_or_zero());
    ((cosine - cos_outer) / (cos_inner - cos_outer).max(1e-4)).clamp(0.0, 1.0)
}

/// How far a modelled froxel may sit from the one the GPU wrote, relative to
/// the larger of the two.
///
/// **Measured, not chosen.** Two evaluations of the same arithmetic in `f32`,
/// one of them a compiled shader free to contract a multiply and an add, so the
/// gap is rounding and not a model. Swept on radv over densities from `0.02` to
/// `0.6` against falloffs from `2` to `40`: the worst lane over the nine ran
/// from `0.000017` to `0.000074`, with no trend in either parameter. This sits
/// an order above the peak of that sweep and is still three orders under the
/// factor either half of a wrong column is out by.
///
/// **Four backends land on the same digit.** The fixture's own medium gives
/// `0.00002` on radv, on lavapipe, on WARP and on Metal — the worst lane sits
/// at a different froxel on each, which is what a rounding difference looks
/// like. So the headroom above the sweep is for a driver nobody has run yet
/// rather than for one of these.
const FROXEL_RELATIVE: f32 = 5e-4;

/// The radiance under which the relative comparison is not asked.
///
/// A froxel that scattered almost nothing is a difference of two small numbers,
/// and a relative bound on one is a bound on the rounding of the other. This is
/// far under any froxel the fixture's medium actually produces, which the test
/// asserts rather than assumes.
const FROXEL_FLOOR: f32 = 1e-4;

/// **The column in the buffer is the column the model says, froxel by froxel.**
///
/// The check the frame-level ones cannot make, for this module's header's
/// reason: a picture compares the composed answer, and the scatter and the scan
/// compose. Here each is wrong on its own or neither is.
///
/// Three things have to hold for it to be evidence rather than arithmetic:
/// the medium has to be doing something (the far end of a column is well short
/// of full transmittance), the sun has to be reaching some froxels and not
/// others (the visibility buffer spans both, which is what makes the term it
/// carries load-bearing), and the radiance has to be above the floor the
/// relative bound is asked over.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_froxel_column_is_the_scan_of_the_slabs_the_medium_scatters() {
    let column = draw_and_read_the_column(COLUMN_FOG, true);
    let count = column.count();
    assert!(
        count > 0,
        "the frame wrote no froxels at all, so this compared nothing"
    );
    let modelled = modelled_column(&column.params, &column.lighting, count);

    let seen: Vec<f32> = column.lighting[..count].iter().map(|lit| lit[3]).collect();
    let darkest = seen.iter().copied().fold(f32::INFINITY, f32::min);
    let brightest = seen.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let thinnest = modelled[..count]
        .iter()
        .map(|cell| cell[3])
        .fold(f32::INFINITY, f32::min);

    let mut worst = 0.0f32;
    let mut worst_at = 0usize;
    let mut compared = 0usize;
    for (froxel, (mine, theirs)) in modelled.iter().zip(&column.froxels).enumerate().take(count) {
        for lane in 0..4 {
            let (a, b) = (mine[lane], theirs[lane]);
            let scale = a.abs().max(b.abs());
            if scale < FROXEL_FLOOR {
                continue;
            }
            let relative = (a - b).abs() / scale;
            if relative > worst {
                worst = relative;
                worst_at = froxel;
            }
            compared += 1;
        }
    }

    eprintln!(
        "{}: froxel column — {count} froxel(s), {compared} lane(s) compared, worst relative gap \
         {worst:.5} at froxel {worst_at}, visibility {darkest:.3}..{brightest:.3}, thinnest \
         modelled transmittance {thinnest:.4}",
        crate::SUITE
    );

    assert!(
        thinnest < 0.9,
        "the deepest slab in the column still transmits {thinnest}, so the medium is doing \
         nearly nothing and this would pass on a column of ones"
    );
    assert!(
        darkest < 0.1 && brightest > 0.9,
        "the visibilities run {darkest} to {brightest}, so the occluder is not splitting the \
         column and the sun term is the same everywhere — the buffer that carries it is not \
         being tested"
    );
    assert!(
        compared > count,
        "only {compared} lane(s) of {count} froxel(s) cleared the {FROXEL_FLOOR} floor, so \
         almost every comparison was skipped"
    );
    assert!(
        worst <= FROXEL_RELATIVE,
        "froxel {worst_at} is {worst} away from the slab-and-scan the parameter block it was \
         built from implies, past the {FROXEL_RELATIVE} two evaluations of this arithmetic \
         differ by — one of the scatter and the scan is wrong, and comparing the composed \
         frame against the closed form cannot say which"
    );
}

/// How far the modelled glow may sit from the lane the GPU wrote, relative to
/// the larger of the two.
///
/// **Measured, not chosen**, on [`FROXEL_RELATIVE`]'s terms and looser than
/// it: a glow is a sum over lights of a length, a normalisation, a fourth
/// power and a phase function, so there is more arithmetic for two `f32`
/// evaluations to disagree in. The fixture's own lights give `0.00008` on
/// radv and `0.00007` on lavapipe, at different froxels; this sits an order
/// above that and three orders under the factor a dropped cone or a dropped
/// phase function is out by.
const GLOW_RELATIVE: f32 = 1e-3;

/// **The glow lanes hold the froxel's list walked at its slice midpoint.**
///
/// `docs/plan/51-volumetrics.md`'s rung 2, checked where it happens: the
/// scatter pass sums the punctual lights in a froxel's cluster list — falloff,
/// cone and phase — at the slab's midpoint, and writes the sum to the buffer
/// beside the column. The host walks every light in the scene from the same
/// point and demands the same number, which says three things at once: that
/// the arithmetic is `mesh.slang`'s, that the phase function is evaluated
/// against the slab's own direction, and that the clustering pass listed every
/// light in every froxel it reaches.
///
/// Three things have to hold for it to be evidence rather than arithmetic:
/// some froxels glow and some are exactly dark (the radius window is a real
/// bound and not a fade to nothing), the spot's cone splits the froxels inside
/// its radius into lit and dark (so `spot_cone` is load-bearing and not a
/// constant one), and the glow clears the floor the relative bound is asked
/// over.
///
/// The host's walk is unoccluded, so the frame is drawn with the atlas at its
/// clear: the pass reads every lamp's tiles and finds nothing in them.
/// `a_lamp_s_glow_stops_at_the_wall_its_map_holds` is what checks that the
/// clear really reads as lit before this leans on it.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn the_glow_in_the_buffer_is_the_froxel_s_list_walked_at_its_midpoint() {
    let column = draw_and_read_the_column(COLUMN_FOG, false);
    let count = column.count();
    assert!(
        count > 0,
        "the frame wrote no froxels at all, so this compared nothing"
    );
    let lights = column_lights();
    let spot = lights
        .iter()
        .find(|light| matches!(light, Light::Spot(_)))
        .expect("the fixture has a spot")
        .row(None);

    let mut worst = 0.0f32;
    let mut worst_at = 0usize;
    let mut compared = 0usize;
    let mut glowing = 0usize;
    let mut dark = 0usize;
    let mut inside_lit = 0usize;
    let mut inside_dark = 0usize;
    for froxel in 0..count {
        let mine = modelled_glow(&column.params, &lights, froxel);
        let [r, g, b, _] = column.lighting[froxel];
        let theirs = Vec3::new(r, g, b);
        if mine.max_element() > FROXEL_FLOOR {
            glowing += 1;
        } else if mine == Vec3::ZERO && theirs == Vec3::ZERO {
            dark += 1;
        }
        for lane in 0..3 {
            let (a, b) = (mine[lane], theirs[lane]);
            let scale = a.abs().max(b.abs());
            if scale < FROXEL_FLOOR {
                continue;
            }
            let relative = (a - b).abs() / scale;
            if relative > worst {
                worst = relative;
                worst_at = froxel;
            }
            compared += 1;
        }

        // The spot's own cone, at the same midpoint: a froxel inside its
        // radius is either in the beam or out of it.
        let Slab { from, to, .. } = slab(&column.params, froxel);
        let position = Vec4::from_array(spot.position);
        let offset = position.truncate() - (from + to) * 0.5;
        if offset.length() < position.w {
            let axis = Vec4::from_array(spot.direction);
            let cone = spot_cone(
                offset / offset.length().max(1e-6),
                axis.truncate(),
                axis.w,
                spot.cos_inner,
            );
            if cone > 0.0 {
                inside_lit += 1;
            } else {
                inside_dark += 1;
            }
        }
    }

    eprintln!(
        "{}: froxel glow — {count} froxel(s), {glowing} glowing, {dark} dark, {compared} lane(s) \
         compared, worst relative gap {worst:.5} at froxel {worst_at}, spot radius splits \
         {inside_lit} lit / {inside_dark} dark",
        crate::SUITE
    );

    assert!(
        glowing > 0 && dark > 0,
        "{glowing} froxel(s) glow and {dark} are dark, so either no light reaches the column or \
         the radius bounds nothing"
    );
    assert!(
        inside_lit > 0 && inside_dark > 0,
        "inside the spot's radius {inside_lit} froxel(s) are in the beam and {inside_dark} out \
         of it, so the cone is not being tested"
    );
    assert!(
        compared > 0,
        "no lane cleared the {FROXEL_FLOOR} floor, so every comparison was skipped"
    );
    assert!(
        worst <= GLOW_RELATIVE,
        "froxel {worst_at}'s glow is {worst} away from its list walked at its midpoint, past \
         the {GLOW_RELATIVE} two evaluations of this arithmetic differ by — the scatter pass \
         is summing something other than mesh.slang's falloff, cone and the medium's phase, \
         or the clustering pass left a light out of a froxel it reaches"
    );
}

/// The fraction of its lit glow a froxel has to lose to the lamps' maps to be
/// counted as occluded.
///
/// A froxel wholly behind the cube from a lamp loses that lamp entirely; one
/// the filter's edge brushes loses a sliver, and one reached by both lamps
/// with only one occluded loses whatever share that lamp had. The count this
/// gates is only asked to be non-zero, so the bar is set where a froxel is
/// plainly in a shadow rather than at its rim — well above the rounding
/// [`GLOW_RELATIVE`] allows, which is what "untouched" is measured by.
const OCCLUDED_FRACTION: f32 = 0.5;

/// **A lamp's glow stops at the wall its map holds.**
///
/// The column drawn twice — the atlas written, and the atlas left at its clear
/// — and the glow lanes compared froxel by froxel. With the atlas clear, every
/// comparison against it comes back lit: the sun lane is exactly one in every
/// froxel, which is checked first because the glow test above rests on it. So
/// the whole difference between the two columns is what the maps hold.
///
/// Occlusion can only take. No froxel glows brighter with the maps than
/// without; some glow plainly darker, because the cube stands between the
/// point light and the far half of the frustum and the spot's cone falls on
/// the cube's top and the floor around it; and most glowing froxels are
/// untouched, which is what says the maps took *somewhere* rather than a
/// coefficient taking everywhere.
#[test]
#[ignore = "needs a real GPU; run crates/crcbl/tests/run-mesh-e2e.sh"]
fn a_lamp_s_glow_stops_at_the_wall_its_map_holds() {
    let lit = draw_and_read_the_column(COLUMN_FOG, false);
    let shadowed = draw_and_read_the_column(COLUMN_FOG, true);
    let count = lit.count();
    assert!(
        count > 0,
        "the frame wrote no froxels at all, so this compared nothing"
    );
    assert!(
        shadowed.count() == count && shadowed.params.eye == lit.params.eye,
        "the two frames cut different grids, so their froxels do not correspond"
    );

    let clear_reads_lit = lit.lighting[..count].iter().all(|lane| lane[3] == 1.0);
    assert!(
        clear_reads_lit,
        "with the atlas unwritten some froxel still saw less than the whole sun, so the clear \
         is not reading as lit and the glow model above is being compared against occluded \
         lamps"
    );

    let mut glowing = 0usize;
    let mut occluded = 0usize;
    let mut untouched = 0usize;
    let mut deepest = 0.0f32;
    let mut brightest_gain = 0.0f32;
    let mut gain_at = 0usize;
    for froxel in 0..count {
        let [r, g, b, _] = lit.lighting[froxel];
        let without = Vec3::new(r, g, b);
        let [r, g, b, _] = shadowed.lighting[froxel];
        let with = Vec3::new(r, g, b);
        if without.max_element() <= FROXEL_FLOOR {
            assert!(
                with.max_element() <= FROXEL_FLOOR,
                "froxel {froxel} glows {with} with the maps and {without} without them, and a \
                 map can only take"
            );
            continue;
        }
        glowing += 1;
        let scale = without.max_element();
        let gain = (with - without).max_element() / scale;
        if gain > brightest_gain {
            brightest_gain = gain;
            gain_at = froxel;
        }
        let loss = (without - with).max_element() / scale;
        if loss >= OCCLUDED_FRACTION {
            occluded += 1;
            deepest = deepest.max(loss);
        } else if loss <= GLOW_RELATIVE {
            untouched += 1;
        }
    }

    eprintln!(
        "{}: froxel occlusion — {count} froxel(s), {glowing} glowing, {occluded} occluded past \
         {OCCLUDED_FRACTION} (deepest {deepest:.3}), {untouched} untouched, brightest gain \
         {brightest_gain:.5} at froxel {gain_at}",
        crate::SUITE
    );

    assert!(
        brightest_gain <= GLOW_RELATIVE,
        "froxel {gain_at} glows {brightest_gain} brighter with its lamps' maps than without \
         them, and a map can only take"
    );
    assert!(
        occluded > 0,
        "no glowing froxel lost {OCCLUDED_FRACTION} of its glow to a map, so the scatter pass \
         is not reading the lamps' tiles and a lamp glows through the cube"
    );
    assert!(
        untouched * 2 > glowing,
        "only {untouched} of {glowing} glowing froxel(s) kept their glow under the maps, so \
         the maps dimmed the column everywhere rather than behind the cube"
    );
}
