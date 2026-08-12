// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s clustered-forward assignment: one compute
// pass that fills a **froxel grid** — screen tiles by depth slices — with the
// lights each froxel touches, so the fragment stage loops over a handful of
// lights rather than over the scene's.
//
// One invocation owns one froxel and walks the whole light list. That is the
// opposite of the obvious arrangement (one invocation per light, scattering into
// the froxels it covers) and it is chosen because **a froxel's list then has one
// writer**: the count needs no atomic, the order is the light list's own order,
// and the overflow count is therefore the same number on every run and every
// device. A scatter would need an atomic append per froxel and would make which
// lights survived the budget a race.
//
// # The depth split is exponential, and the reversed-Z buffer is not involved
//
// Slices are Olsson and Assarsson's `z(k) = near * (far / near)^(k / slices)`.
// A uniform split over `near..far` is the tempting one and it is useless: at a
// 0.1 m near plane and a 1000 m far, the first slice would be 41 m deep and hold
// every light in the scene, which is Forward+ with extra steps.
//
// The quantity being split is the **view depth**, `clip.w` — not the depth
// buffer. This engine is reversed-Z, so the buffer holds 1.0 at the near plane
// and 0.0 at infinity: an index taken from `SV_Position.z` would run *backwards*
// and be distributed hyperbolically on top, which is a slice numbering that
// looks plausible and puts every fragment in the wrong list. `mesh.slang`'s
// fragment stage recomputes `clip.w` from the frame's `view_proj` for exactly
// this reason, and `depth_row` below is the same row of the same matrix so the
// two sides cannot disagree.
//
// # Conservative, and where the bound comes from
//
// A light that touches a froxel and is missing from its list is a seam — a hard
// edge in the middle of a lit surface that reads as a shading bug. So every
// approximation here is one that can only *add* froxels:
//
// * A froxel is the convex hull of its eight corners (four tile rays cut by two
//   depth planes), so the axis-aligned box of those corners contains it exactly.
//   Testing against the box accepts a light the froxel itself misses; it never
//   rejects one it touches.
// * A light's influence is bounded by `GpuLight::position.w`, and that bound is
//   real rather than a fade cut short: the falloff `mesh.slang` applies reaches
//   exactly zero at that radius. A sphere-versus-box test is therefore a test of
//   the whole of the light.
// * A spot is bounded by its cone **as well as** its sphere, and has to pass
//   both. The sphere alone lists a narrow spot in every froxel behind it and
//   beside it; the cone is tested against the *bounding sphere of the froxel's
//   box*, which contains the box, so it too can only add froxels. See
//   `cone_touches_sphere`, which is where the three rejections are argued one at
//   a time — a cone test that is slightly too tight leaves a froxel that should
//   list the light without it, and that draws a hard seam across a lit surface
//   which reads as a shading bug rather than a culling one.
// * The last slice's far side is unbounded (`FLT_MAX`), so a light past
//   `CLUSTER_FAR` is still assigned to the froxels a fragment out there reads.
//   The grid's range is where the *resolution* stops, not where the lighting
//   does.
//
// The box is built per (froxel, light) rather than once per froxel, and that is
// what makes an unbounded slice affordable: the froxel is first cut down to the
// light's own depth band, so the box tested is the part of the froxel the light
// could possibly reach rather than a box stretching to `FLT_MAX`.
//
// # There is no shared header, deliberately
//
// `cull.slang`'s header carries the full argument: the compile script hashes one
// source per artifact, so an `#include` would be a file nothing downstream
// notices changing. The structs below are re-declared and
// `crcbl_shaders::cull`'s `the_shared_structs_are_declared_identically_in_every_shader`
// compares them character for character.

/// One light, matching `struct GpuLight` in `mesh.slang` and
/// `crcbl_shaders::light::GpuLight`.
struct GpuLight
{
    /// World-space position in `xyz`, influence radius in `w`.
    float4 position;
    /// Colour premultiplied by intensity in `rgb`; `w` unused.
    float4 color;
    /// Towards the light for a directional; the cone axis for a spot. `w` is the
    /// cosine of a spot's outer half-angle.
    float4 direction;
    uint kind;
    /// The cosine of a spot's inner half-angle.
    float cos_inner;
    /// Which slot of `FrameUniforms::light_view_proj` this light occludes
    /// through, or `NO_SHADOW_SLOT` if it was given none.
    ///
    /// **Read by nothing here.** Which lights are in a froxel's list has nothing
    /// to do with which of them cast shadows — an unshadowed light lights
    /// exactly as far as a shadowed one — so this pass carries the field for the
    /// layout's sake and `mesh.slang`'s fragment stage is what reads it.
    uint shadow_slot;
    uint pad1;
};

/// `GpuLight::kind` for the sun: no position, no falloff, and in every froxel.
static const uint KIND_DIRECTIONAL = 0;

/// `GpuLight::kind` for a point light.
static const uint KIND_POINT = 1;

/// `GpuLight::kind` for a spot light.
static const uint KIND_SPOT = 2;

/// How many light indices one froxel's list holds, matching
/// `crcbl_shaders::light::CLUSTER_LIGHT_CAPACITY`. **The budget.**
static const uint CLUSTER_LIGHT_CAPACITY = 16;

/// Words one froxel occupies: its count, then `CLUSTER_LIGHT_CAPACITY` indices.
/// Matching `crcbl_shaders::light::CLUSTER_STRIDE`.
static const uint CLUSTER_STRIDE = 17;

/// The statistics word dropped assignments are counted into, matching
/// `crcbl_shaders::light::CLUSTER_OVERFLOW_WORD`.
static const uint CLUSTER_OVERFLOW_WORD = 2;

/// Depth slices a perspective grid has, matching
/// `crcbl_shaders::light::CLUSTER_DEPTH_SLICES`.
static const uint CLUSTER_DEPTH_SLICES = 24;

/// The view depth the first slice starts at, matching
/// `crcbl_shaders::light::CLUSTER_NEAR`.
static const float CLUSTER_NEAR = 0.1;

/// The view depth the last slice's *resolution* ends at, matching
/// `crcbl_shaders::light::CLUSTER_FAR`. Not a cull distance — see the header.
static const float CLUSTER_FAR = 1000.0;

/// The reversed-Z near plane in NDC, matching `crcbl_hal::depth::NEAR`.
static const float DEPTH_NEAR = 1.0;

/// The reversed-Z far plane in NDC, matching `crcbl_hal::depth::FAR`.
static const float DEPTH_FAR = 0.0;

/// Stands in for an unbounded far side. `FLT_MAX` rather than an infinity: the
/// only thing done with it is a `min` against a finite depth, and an infinity
/// would still be finite after that while costing every target a special case.
static const float DEPTH_UNBOUNDED = 3.402823466e+38;

/// What this pass cannot derive for itself, matching
/// `crcbl_shaders::light::ClusterParams`.
struct LightClusterParams
{
    /// Clip → world, for unprojecting a tile's screen corners.
    float4x4 inverse_view_proj;
    /// World-space eye in `xyz`; `w` unused.
    float4 eye;
    /// Row 3 of the view-projection, so `dot(depth_row, float4(p, 1))` is `p`'s
    /// view depth.
    float4 depth_row;
    uint grid_x;
    uint grid_y;
    /// `CLUSTER_DEPTH_SLICES` for a perspective camera, `1` for an orthographic
    /// one.
    uint slices;
    uint light_count;
    /// The viewport, in pixels, so a tile's NDC rectangle is its *pixel*
    /// rectangle and matches the one the fragment stage divides down.
    uint viewport_x;
    uint viewport_y;
    /// `1` for a perspective camera, `0` for an orthographic one — see the
    /// orthographic branch below.
    uint perspective;
    /// How many pixels wide and tall one tile is, matching
    /// `FrameUniforms::cluster_grid`'s `w`.
    ///
    /// A parameter rather than the constant it usually equals, so a viewport too
    /// large for the grid buffer gets *coarser* tiles rather than a grid that
    /// runs off the end of it. The host is what doubles it; both sides read it
    /// from the frame.
    uint tile_pixels;
};

[[vk::binding(0, 0)]]
ConstantBuffer<LightClusterParams> params;

[[vk::binding(1, 0)]]
StructuredBuffer<GpuLight> lights;

/// The grid: `CLUSTER_STRIDE` words per froxel, count first. Written here and
/// read by `mesh.slang`'s fragment stage.
///
/// **One writer per froxel**, so nothing here is atomic and the list's order is
/// the light list's order. See the header.
[[vk::binding(2, 0)]]
RWStructuredBuffer<uint> cluster_lights;

/// The frame's culling statistics, which this pass adds its dropped assignments
/// to at `CLUSTER_OVERFLOW_WORD`.
///
/// The buffer `cull.slang` counts surviving instances into, two elements along —
/// topic 03 §3.6's delayed readback ring is that one buffer, and a counter of
/// its own would be a second readback. `Atomic<uint>` rather than
/// `InterlockedAdd` for that file's reason: spelled as a type it lowers on every
/// target.
[[vk::binding(3, 0)]]
RWStructuredBuffer<Atomic<uint> > cull_stats;

/// A point's view depth — the `w` a perspective divide would use.
float view_depth(float3 point)
{
    return dot(params.depth_row, float4(point, 1.0));
}

/// The world-space point at NDC `(ndc, depth)`.
float3 unproject(float2 ndc, float depth)
{
    float4 world = mul(params.inverse_view_proj, float4(ndc, depth, 1.0));
    return world.xyz / world.w;
}

/// Whether a spot's cone reaches a sphere — the test that stops a narrow spot
/// being listed in every froxel its *radius* covers.
///
/// **Conservative, and it has to be shown to be.** The three rejections are the
/// three ways a sphere can miss a cone, and each is written with the sphere's
/// own radius as slack, so a sphere that genuinely touches the cone passes all
/// three:
///
/// * behind the apex, further back than the sphere reaches;
/// * past the end of the cone's range, likewise;
/// * outside the cone's opening. In the half-plane spanned by the axis and the
///   sphere's centre, the cone's edge is the line through the apex at the outer
///   half-angle, and `cos_outer * perpendicular - along * sin_outer` is the
///   distance from the centre to it, signed positive on the outside. A sphere
///   holding any point of the cone holds a point at distance zero or less, and
///   its centre is then within `radius` of the line.
///
/// The caller passes the **bounding sphere of the froxel's box**, which contains
/// the box, so a cone touching the box touches the sphere. That keeps the whole
/// statement conservative rather than only this half of it. A cone test that is
/// even slightly too tight leaves froxels that should list the light and do not,
/// which draws a hard seam across a lit surface and reads as a shading bug.
bool cone_touches_sphere(
    float3 apex, float3 axis, float cos_outer, float range, float3 center, float radius)
{
    float3 offset = center - apex;
    float along = dot(offset, axis);
    if (along < -radius || along > range + radius)
    {
        return false;
    }
    // `saturate` before the root: the cosine arrives from a host that took it of
    // an angle, and a value a hair over one would otherwise be a `sqrt` of a
    // negative and a `NaN` that compares false against everything.
    float sin_outer = sqrt(saturate(1.0 - cos_outer * cos_outer));
    float perpendicular = sqrt(max(dot(offset, offset) - along * along, 0.0));
    return cos_outer * perpendicular - along * sin_outer <= radius;
}

/// The view depth slice `index` starts at.
///
/// `CLUSTER_DEPTH_SLICES` rather than `params.slices`, so this is literally the
/// expression `crcbl_shaders::light::slice_scale` inverts. An orthographic frame
/// runs with one slice and does not reach here.
float slice_start(uint index)
{
    return CLUSTER_NEAR * pow(CLUSTER_FAR / CLUSTER_NEAR, float(index) / float(CLUSTER_DEPTH_SLICES));
}

[shader("compute")]
[numthreads(64, 1, 1)]
void computeMain(uint3 thread: SV_DispatchThreadID)
{
    uint froxel = thread.x;
    uint tiles = params.grid_x * params.grid_y;
    if (froxel >= tiles * params.slices)
    {
        return;
    }

    uint tile_x = froxel % params.grid_x;
    uint tile_y = (froxel / params.grid_x) % params.grid_y;
    uint slice = froxel / tiles;

    // The tile's NDC rectangle, from its *pixel* rectangle, because that is what
    // the fragment stage divides down. The last tile in a row and column runs
    // past the viewport edge; left unclamped, which only makes the froxel larger
    // and so only makes the test below more conservative.
    float2 viewport = float2(float(params.viewport_x), float(params.viewport_y));
    float2 pixel_min = float2(float(tile_x), float(tile_y)) * float(params.tile_pixels);
    float2 pixel_max = pixel_min + float(params.tile_pixels);
    // Y-up NDC, and pixel row zero is the *top* of the screen — the flip
    // `crcbl_render`'s projections push into the viewport rather than into the
    // matrix, so the vertical bounds swap on the way in.
    float2 ndc_min = float2(pixel_min.x / viewport.x * 2.0 - 1.0, 1.0 - pixel_max.y / viewport.y * 2.0);
    float2 ndc_max = float2(pixel_max.x / viewport.x * 2.0 - 1.0, 1.0 - pixel_min.y / viewport.y * 2.0);

    // The four corner rays, as points on the near plane. A perspective froxel's
    // corner at any view depth is on the ray from the eye through one of these,
    // which is what makes cutting the froxel to a light's depth band four
    // multiplies rather than four unprojections.
    float3 corner_near[4];
    corner_near[0] = unproject(float2(ndc_min.x, ndc_min.y), DEPTH_NEAR);
    corner_near[1] = unproject(float2(ndc_max.x, ndc_min.y), DEPTH_NEAR);
    corner_near[2] = unproject(float2(ndc_min.x, ndc_max.y), DEPTH_NEAR);
    corner_near[3] = unproject(float2(ndc_max.x, ndc_max.y), DEPTH_NEAR);

    // **An orthographic camera is a different construction, not a degenerate
    // case of the perspective one.** Its rays do not converge on an eye and its
    // `clip.w` is 1 everywhere, so it has no view depth for a slice index to
    // come from. It therefore runs with one slice per tile spanning the whole
    // depth range, whose far corners are an ordinary unprojection of the far
    // plane — finite, unlike a perspective camera's, which is at infinity.
    bool perspective = params.perspective != 0;
    float3 ortho_lo = float3(0.0, 0.0, 0.0);
    float3 ortho_hi = float3(0.0, 0.0, 0.0);
    float eye_to_near = 1.0;
    if (perspective)
    {
        // Every corner is on the same near plane, so one depth serves all four.
        eye_to_near = view_depth(corner_near[0]);
    }
    else
    {
        ortho_lo = corner_near[0];
        ortho_hi = corner_near[0];
        for (uint corner = 0; corner < 4; ++corner)
        {
            float3 far_point = unproject(
                float2(corner == 1 || corner == 3 ? ndc_max.x : ndc_min.x,
                       corner < 2 ? ndc_min.y : ndc_max.y),
                DEPTH_FAR);
            ortho_lo = min(ortho_lo, min(corner_near[corner], far_point));
            ortho_hi = max(ortho_hi, max(corner_near[corner], far_point));
        }
    }

    // This froxel's depth band. The last slice's far side is unbounded, so a
    // light past `CLUSTER_FAR` is still listed for the fragments out there.
    float band_lo = perspective ? slice_start(slice) : 0.0;
    float band_hi = (!perspective || slice + 1 >= params.slices)
        ? DEPTH_UNBOUNDED
        : slice_start(slice + 1);

    uint base = froxel * CLUSTER_STRIDE;
    uint kept = 0;
    uint dropped = 0;
    for (uint index = 0; index < params.light_count; ++index)
    {
        GpuLight light = lights[index];
        bool touches;
        if (light.kind == KIND_DIRECTIONAL)
        {
            // The sun affects every froxel, which is the whole point of it being
            // a row: the fragment stage walks one list and never asks what kind
            // of frame it is in.
            touches = true;
        }
        else
        {
            float3 center = light.position.xyz;
            float radius = light.position.w;
            float3 box_lo;
            float3 box_hi;
            if (perspective)
            {
                // Cut the froxel down to the light's own depth band before
                // bounding it. Conservative — the part removed is depth the
                // light cannot reach — and it is what keeps the last slice's
                // unbounded far side from producing an unbounded box.
                float depth = view_depth(center);
                float light_lo = depth - radius;
                float light_hi = depth + radius;
                if (light_hi < band_lo || light_lo > band_hi)
                {
                    continue;
                }
                float cut_lo = max(band_lo, light_lo);
                float cut_hi = min(band_hi, light_hi);
                // A light straddling or behind the eye clamps to the froxel's
                // own near side rather than reflecting through it.
                cut_lo = max(cut_lo, band_lo);
                cut_hi = max(cut_hi, cut_lo);

                float3 first = params.eye.xyz
                    + (corner_near[0] - params.eye.xyz) * (cut_lo / eye_to_near);
                box_lo = first;
                box_hi = first;
                for (uint corner = 0; corner < 4; ++corner)
                {
                    float3 ray = corner_near[corner] - params.eye.xyz;
                    float3 at_lo = params.eye.xyz + ray * (cut_lo / eye_to_near);
                    float3 at_hi = params.eye.xyz + ray * (cut_hi / eye_to_near);
                    box_lo = min(box_lo, min(at_lo, at_hi));
                    box_hi = max(box_hi, max(at_lo, at_hi));
                }
            }
            else
            {
                box_lo = ortho_lo;
                box_hi = ortho_hi;
            }

            // Sphere against the box: the squared distance from the centre to
            // the nearest point of the box.
            float3 closest = clamp(center, box_lo, box_hi);
            float3 offset = center - closest;
            touches = dot(offset, offset) <= radius * radius;

            // And, for a spot, its cone against the box's own bounding sphere.
            // **Both rather than either**: each of the two is conservative on
            // its own, so a light has to fail one of them to be rejected and
            // neither can reject a froxel the other would have kept.
            if (touches && light.kind == KIND_SPOT)
            {
                float3 box_center = (box_lo + box_hi) * 0.5;
                float box_radius = length(box_hi - box_lo) * 0.5;
                touches = cone_touches_sphere(
                    center, light.direction.xyz, light.direction.w, radius, box_center, box_radius);
            }
        }

        if (!touches)
        {
            continue;
        }
        if (kept < CLUSTER_LIGHT_CAPACITY)
        {
            cluster_lights[base + 1 + kept] = index;
            kept += 1;
        }
        else
        {
            // **Counted, never silently dropped.** This is the number
            // `docs/plan/18-render-features.md` asks a scene that outgrew the
            // budget to be able to state.
            dropped += 1;
        }
    }

    cluster_lights[base] = kept;
    if (dropped > 0)
    {
        cull_stats[CLUSTER_OVERFLOW_WORD].add(dropped);
    }
}
