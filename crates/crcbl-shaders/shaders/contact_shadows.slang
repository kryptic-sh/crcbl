// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/45-shadows.md`'s screen-space contact shadows: the pass that
// marches the depth prepass towards the sun and writes one `R8Unorm` channel
// the forward pass multiplies its **directional shadow term** by.
//
// # What it is for, and what no shadow map can do instead
//
// A cascade texel is a finite patch of the world, and the sun's shadow can only
// resolve a gap that patch can see. The sliver where a foot meets the floor or a
// book meets a shelf is finer than that patch at every cascade the atlas holds,
// so a bias large enough to stop acne is also large enough to detach that
// contact — the shadow starts a texel away from the thing casting it and the
// object reads as hovering. `mesh.slang`'s `cascade_visibility` carries the
// biases and says the same thing from the other side. This pass closes that gap
// with evidence the atlas does not have: the depth prepass, at frame resolution,
// where the contact is a few pixels wide.
//
// # It is `ssr.slang`'s march, one level shallower
//
// Every helper below — `depth_at`, `view_position`, `normal_at`, `pixel_of`,
// `ndc_of`, `view_z_of`, `thickness_at`, `cell_exit` — is the reflection march's,
// spelled again because Slang has no `#include` and the manifest hashes one
// source per artifact (`ssr.slang`'s own header argues that at length). The
// loop is that file's loop with `level` held at zero: one depth texel per step,
// a perspective-correct view `z` interpolated through `1 / w`, and the same
// thickness test deciding whether a crossing is a surface in the way or a hole
// in the evidence.
//
// **The Hi-Z pyramid is deliberately not read.** A hierarchical march exists to
// skip empty space over a ray that may cross the whole frame; this ray is
// [`MAX_STEPS`] texels long, so every cell it would skip is a cell it was about
// to leave anyway — the pyramid would cost a level descent per crossing and buy
// nothing. `docs/plan/45-shadows.md` lists the pyramid among the things the tree
// already has; that it is available is not a reason to climb it.
//
// # Determinism: `ssr.slang`'s argument, and it binds harder here
//
// A march has no denominator to divide a flipped sample by — the first crossing
// *is* the answer. So, exactly as in that file:
//
// * **No jitter and no hash of any kind.** A dither is a per-pixel amplifier of
//   the float differences a golden cannot absorb. The march steps one depth
//   texel at a time, which is the sampling rate the evidence actually has, so
//   there is no undersampling for a dither to hide in the first place.
// * **Every weight is continuous and reaches zero where the decision is
//   fragile**: a crossing in the last of the ray's reach is on
//   [`DISTANCE_FADE`]'s ramp, one near the frame edge on [`BORDER_FADE`]'s, one
//   at the limit of what [`thickness_at`] will call a surface on the bound's,
//   and a surface turning away from the sun is on [`GRAZING_FADE`]'s.
//
// # The off-switch is a texel, not a branch
//
// `crcbl_render::forward` binds a 1×1 white `R8Unorm` image when it records no
// contact-shadow pass, exactly as it does for the occlusion channel, and
// `mesh.slang`'s `contact_at` clamps its `Load` into whatever it was given. So a
// frame without this effect multiplies its sun by 1.0 and this file has no
// switched-off arm to keep working.

/// `crcbl_shaders::contact_shadows::ContactShadowParams` is the Rust mirror and
/// `crcbl_render::contact_shadows` is what writes it.
struct ContactShadowParams
{
    /// Clip → view. The march is a view-space ray walked in screen space, so
    /// this is what turns a depth texel back into a point the ray can be
    /// compared against.
    float4x4 inv_proj;
    /// View → clip, for projecting the ray's start and its direction.
    float4x4 proj;
    /// `xyz` is the unit view-space direction **towards** the sun, matching the
    /// `direction` a `KIND_DIRECTIONAL` row in `mesh.slang`'s light list
    /// carries. `w` is zero and unread.
    float4 to_light;
};

/// **No sampler in this layout.** Every read below is a `Load`, which needs no
/// UV, no texel-centre arithmetic and no filter for four rasterisers to disagree
/// about — `ssao.slang`'s binding block makes the same argument.
[[vk::binding(0, 0)]]
ConstantBuffer<ContactShadowParams> camera;
/// The depth prepass, `DepthTexture2D` for the seam's `SampleType::Depth` — the
/// only sample type WebGPU will bind a `D32Float` view through.
[[vk::binding(1, 0)]]
DepthTexture2D scene_depth;

/// The reversed-Z far plane: the value the prepass clears to, and the depth at
/// which an infinite projection's `clip.w` reaches zero.
/// `crcbl_shaders::contact_shadows::DEPTH_FAR` is the Rust mirror.
static const float DEPTH_FAR = 0.0;

/// The channel a fragment nothing shadows gets, and the value the renderer's
/// 1×1 placeholder holds.
static const float LIT = 1.0;

/// How far the ray reaches, in world units.
///
/// **A world length rather than a screen length**, so a contact is the same size
/// in the scene whether the camera is beside it or across the room — a
/// screen-space reach would grow the shadow as the camera closed on it, which is
/// the artefact that makes a contact term read as dirt.
///
/// Swept on `apps/lantern`'s room at 1920×1080 before it was fixed here; see
/// `docs/plan/45-shadows.md`. Below a tenth of a metre the crate legs' contact
/// is still detached at the near cascade's bias; past half a metre the term
/// stops being a contact and starts competing with the cascade it is meant to
/// complete, darkening whole corners the atlas already answers for.
static const float RAY_LENGTH = 0.25;

/// Depth texels the march may cross.
///
/// One step per texel — see the header on why the pyramid is not climbed — so
/// this is both the step budget and the reach in pixels, and [`MAX_REACH`] is
/// the second name for it that the travel clamp wants.
static const uint MAX_STEPS = 16u;

/// The pixel reach the step budget can actually walk.
///
/// One less than [`MAX_STEPS`] because the first step only crosses the remainder
/// of the pixel the ray starts inside. Clamping `travel` to this rather than
/// letting the loop run out of steps is what makes the reach the one tunable:
/// the march always ends at a stated distance instead of wherever the budget
/// happened to expire.
static const float MAX_REACH = float(MAX_STEPS) - 1.0;

/// Pixels of reach under which the pass gives up and reports lit.
///
/// A contact narrower than this is a shadow the frame has no texels to hold —
/// `ssao.slang`'s `MIN_RADIUS_PIXELS` is the same floor for the same reason, and
/// it is what keeps distant geometry from paying for a term it cannot show.
static const float MIN_REACH = 2.0;

/// How far the ray's start is lifted off the surface along the reconstructed
/// normal, as a fraction of the surface's view-space depth.
///
/// `ssr.slang`'s constant and its reason: proportional to depth because a
/// fixed offset would be sub-texel near the eye and metres away at the far
/// plane.
static const float NORMAL_OFFSET = 0.005;

/// The least thickness a depth texel is treated as having, as a fraction of its
/// own view-space depth. `ssr.slang`'s constant.
static const float THICKNESS_FLOOR = 0.02;

/// The fraction of a cell the boundary crossing is nudged past, so a step that
/// lands exactly on an edge still advances. `ssr.slang`'s constant.
static const float BOUNDARY_NUDGE = 0.00390625;

/// The last fraction of the ray's reach over which a crossing fades out.
///
/// A hit at the very end of the reach is the one a step either side of it would
/// have missed, so it arrives with the least weight rather than the most.
static const float DISTANCE_FADE = 0.25;

/// The fraction of the frame at its edge over which a crossing fades out. Past
/// the border there is no evidence at all, and a shadow that ended abruptly
/// there would swim as the camera turned.
static const float BORDER_FADE = 0.15;

/// The `N · L` below which the term fades back to lit.
///
/// `mesh.slang`'s `sun_visibility` returns fully lit at `n_dot_l <= 0` — the
/// surface is already black from the cosine and it is where self-shadowing acne
/// is worst — and this is the continuous approach to that same answer, so the
/// terminator carries no step of its own.
static const float GRAZING_FADE = 0.1;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

float depth_at(int2 pixel, int2 extent)
{
    int2 clamped = clamp(pixel, int2(0, 0), extent - int2(1, 1));
    return scene_depth.Load(int3(clamped, 0));
}

/// The two rows of the inverse projection that `depth` alone reaches: the
/// unprojected `z` and the `w` a caller divides it by.
///
/// **Split out of [`unproject`] so that no caller computes a component it
/// discards.** `ssr.slang`'s `view_z_of` and the two blurs' `view_z` want the
/// depth alone, and routing them through the full unprojection leaves the other
/// two components to a dead-code pass that may or may not run — a cost that
/// depends on the driver rather than on this file. This way every component
/// either function computes is one its caller reads, which is a property rather
/// than a hope.
///
/// **Not a measured win.** The split was made after the `ssr` pass read
/// 0.027 ms slower without it, and a paired re-measurement did not confirm
/// that: passes with no code change at all move by as much between builds on
/// the machine it was read on. What stands is the argument above, not a number.
float2 unproject_z(float depth)
{
    return float2(
        camera.inv_proj[2][2] * depth + camera.inv_proj[2][3],
        camera.inv_proj[3][2] * depth + camera.inv_proj[3][3]);
}

/// The inverse projection applied to one clip-space point, with the terms that
/// are structurally zero left out.
///
/// **The same value as `mul(camera.inv_proj, float4(ndc, depth, 1.0))`, not an
/// approximation of it.** Both projections `crcbl_render::camera::Projection`
/// builds — an infinite reversed-Z perspective and a reversed orthographic box
/// — invert to a matrix whose only non-zero entries are the eight read below,
/// so most of the general product's sixteen multiplies are against an exact
/// zero, and adding an exact zero is exact. What holds that true of a
/// projection added later is a pair of tests rather than this comment —
/// `crcbl_render::camera`'s
/// `every_projection_inverts_to_the_sparsity_the_screen_space_shaders_read`
/// for the pattern and
/// `the_screen_space_unprojection_is_the_full_product_for_that_sparsity` for
/// the four terms below being the whole of it — because a projection that
/// broke either would draw a wrong picture rather than fail to compile.
///
/// `camera.inv_proj[r][c]` is row `r`, column `c` — the same logical element
/// `mul` reads, whatever layout the target packs the block in.
///
/// **The call count is the reason it is worth writing out.** `ssao.slang`
/// unprojects once per horizon tap, once per neighbour the normal takes and
/// once for the centre, so a pixel pays for the dropped terms over and over
/// before the pass has shaded anything.
float4 unproject(float2 ndc, float depth)
{
    float2 depth_row = unproject_z(depth);
    return float4(
        camera.inv_proj[0][0] * ndc.x + camera.inv_proj[0][3],
        camera.inv_proj[1][1] * ndc.y + camera.inv_proj[1][3],
        depth_row.x,
        depth_row.y);
}

float3 view_position(int2 pixel, float depth, float2 extent)
{
    // Pixel centre → NDC. `+Y` is up in this engine's clip space on every target
    // — `crcbl-vk` submits a negative-height viewport so that Vulkan agrees with
    // the other three — while `SV_Position.y` counts down the framebuffer. Hence
    // the subtraction on `y` and not on `x`; `tonemap.slang`'s `-2.0` is the same
    // fact seen from the vertex stage.
    float2 ndc = float2(
        (float(pixel.x) + 0.5) / extent.x * 2.0 - 1.0,
        1.0 - (float(pixel.y) + 0.5) / extent.y * 2.0);
    float4 view = unproject(ndc, depth);
    return view.xyz / view.w;
}

float3 normal_at(int2 pixel, float3 centre, int2 extent, float2 size)
{
    float3 left = view_position(pixel + int2(-1, 0), depth_at(pixel + int2(-1, 0), extent), size);
    float3 right = view_position(pixel + int2(1, 0), depth_at(pixel + int2(1, 0), extent), size);
    float3 up = view_position(pixel + int2(0, -1), depth_at(pixel + int2(0, -1), extent), size);
    float3 down = view_position(pixel + int2(0, 1), depth_at(pixel + int2(0, 1), extent), size);

    float3 horizontal = abs(right.z - centre.z) < abs(centre.z - left.z)
        ? right - centre
        : centre - left;
    float3 vertical = abs(down.z - centre.z) < abs(centre.z - up.z)
        ? down - centre
        : centre - up;

    // `cross(vertical, horizontal)` and not the other order. View space is
    // right-handed with the eye looking down `-Z`; `horizontal` runs along `+X`
    // and `vertical` — one framebuffer row *down* — runs along `-Y`, so
    // `cross(-Y, +X)` is `+Z`, which faces the camera. The other order gives a
    // normal pointing into the surface, and a hemisphere built on it samples the
    // solid side of every wall: the frame goes uniformly dark rather than
    // obviously wrong, which is why `render_e2e`'s ratio exists.
    return normalize(cross(vertical, horizontal));
}

float2 pixel_of(float2 ndc, float2 size)
{
    return float2((ndc.x * 0.5 + 0.5) * size.x, (0.5 - ndc.y * 0.5) * size.y);
}

float2 ndc_of(float2 at, float2 size)
{
    return float2(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0);
}

float thickness_at(float advance, float depth)
{
    return max(advance, abs(depth) * THICKNESS_FLOOR);
}

float view_z_of(float depth)
{
    float2 view = unproject_z(depth);
    return view.x / view.y;
}

float cell_exit(float2 at, float2 forward, float size, float reach)
{
    float2 edge = float2(
        forward.x > 0.0 ? (floor(at.x / size) + 1.0) * size : floor(at.x / size) * size,
        forward.y > 0.0 ? (floor(at.y / size) + 1.0) * size : floor(at.y / size) * size);
    float nudge = size * BOUNDARY_NUDGE;
    float along_x = abs(forward.x) < 1e-6
        ? reach
        : ((edge.x + (forward.x > 0.0 ? nudge : -nudge)) - at.x) / forward.x;
    float along_y = abs(forward.y) < 1e-6
        ? reach
        : ((edge.y + (forward.y > 0.0 ? nudge : -nudge)) - at.y) / forward.y;
    return max(min(along_x, along_y), nudge);
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    scene_depth.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));
    int2 pixel = int2(input.position.xy);

    float depth = depth_at(pixel, extent);
    if (depth <= DEPTH_FAR)
    {
        // The far plane has no surface, so there is nothing here for the sun to
        // be occluded from. `ssao.slang` leaves early at the same value.
        return LIT;
    }

    float3 origin = view_position(pixel, depth, size);
    float3 normal = normal_at(pixel, origin, extent, size);
    float3 ray = camera.to_light.xyz;
    float n_dot_l = dot(normal, ray);
    float facing = saturate(n_dot_l / GRAZING_FADE);
    if (facing <= 0.0)
    {
        return LIT;
    }

    float3 start = origin + normal * (abs(origin.z) * NORMAL_OFFSET);
    float4 clip_start = mul(camera.proj, float4(start, 1.0));
    float4 clip_ray = mul(camera.proj, float4(ray, 0.0));
    if (clip_start.w <= 0.0)
    {
        return LIT;
    }

    float2 at_start = pixel_of(clip_start.xy / clip_start.w, size);
    float2 ndc_rate =
        (clip_ray.xy * clip_start.w - clip_start.xy * clip_ray.w) / (clip_start.w * clip_start.w);
    float2 screen_rate = float2(ndc_rate.x * 0.5 * size.x, -ndc_rate.y * 0.5 * size.y);
    float rate = length(screen_rate);
    if (rate < 1e-6)
    {
        // The sun is along the view axis at this pixel: the ray has no screen
        // extent to march and every step would read the texel it started in.
        return LIT;
    }
    float2 forward = screen_rate / rate;

    // The reach, narrowed by each thing that can end the ray sooner: the step
    // budget, the world length, the frame's edge, and the near plane or the
    // vanishing point. Each clamp is `ssr.slang`'s, and the first two are this
    // file's own.
    float travel = MAX_REACH;
    float4 clip_end = mul(camera.proj, float4(start + ray * RAY_LENGTH, 1.0));
    if (clip_end.w > 0.0)
    {
        float2 at_end_world = pixel_of(clip_end.xy / clip_end.w, size);
        travel = min(travel, max(dot(at_end_world - at_start, forward), 0.0));
    }
    if (forward.x > 0.0)
    {
        travel = min(travel, (size.x - 1.0 - at_start.x) / forward.x);
    }
    else if (forward.x < 0.0)
    {
        travel = min(travel, -at_start.x / forward.x);
    }
    if (forward.y > 0.0)
    {
        travel = min(travel, (size.y - 1.0 - at_start.y) / forward.y);
    }
    else if (forward.y < 0.0)
    {
        travel = min(travel, -at_start.y / forward.y);
    }
    if (clip_ray.w < 0.0)
    {
        // The ray leaves through the near plane. Everything past that crossing
        // projects from behind the eye, so the march stops there.
        float4 on_near = mul(camera.inv_proj, float4(0.0, 0.0, 1.0, 1.0));
        float near = -on_near.z / on_near.w;
        float when = (near - clip_start.w) / clip_ray.w;
        float4 clip_near = clip_start + clip_ray * when;
        float2 at_near = pixel_of(clip_near.xy / clip_near.w, size);
        travel = min(travel, max(dot(at_near - at_start, forward), 0.0));
    }
    travel = max(travel, 0.0);
    if (travel < MIN_REACH)
    {
        // Sub-pixel, or nothing left to walk. A contact the frame has no texels
        // to hold is one this pass reports as lit rather than guesses at.
        return LIT;
    }

    float2 at_end = at_start + forward * travel;
    float2 ndc_end = ndc_of(at_end, size);
    float when_end = abs(forward.x) >= abs(forward.y)
        ? (ndc_end.x * clip_start.w - clip_start.x) / (clip_ray.x - ndc_end.x * clip_ray.w)
        : (ndc_end.y * clip_start.w - clip_start.y) / (clip_ray.y - ndc_end.y * clip_ray.w);
    if (!(when_end > 0.0) || !isfinite(when_end))
    {
        return LIT;
    }

    float inverse_w_start = 1.0 / clip_start.w;
    float w_end = clip_start.w + when_end * clip_ray.w;
    float inverse_w_end = 1.0 / w_end;
    float over_w_start = start.z * inverse_w_start;
    float over_w_end = (start.z + when_end * ray.z) * inverse_w_end;

    float at_travel = min(cell_exit(at_start, forward, 1.0, travel), travel);
    float entry_z = start.z;
    // The normal offset put the start in front of the surface it came from, so
    // the march opens in front of the depth buffer by construction. Spelling it
    // as the same subtraction the loop makes keeps the first crossing on the
    // same footing as every later one.
    float previous_gap = start.z - origin.z;
    for (uint step = 0u; step < MAX_STEPS; ++step)
    {
        float2 at = at_start + forward * at_travel;
        float exit_travel = min(at_travel + cell_exit(at, forward, 1.0, travel), travel);
        float2 exit_at = at_start + forward * exit_travel;
        float along = exit_travel / travel;
        float exit_z =
            lerp(over_w_start, over_w_end, along) / lerp(inverse_w_start, inverse_w_end, along);
        float cell_depth = depth_at(int2(floor(at)), extent);
        // A texel the geometry never covered is a hole in the evidence rather
        // than a surface at infinity, so the ray passes through it in front.
        float gap = cell_depth <= DEPTH_FAR ? 1.0 : exit_z - view_z_of(cell_depth);
        if (gap <= 0.0 && previous_gap > 0.0)
        {
            float behind = -gap;
            float thickness = thickness_at(abs(exit_z - entry_z), exit_z);
            if (behind <= thickness)
            {
                float crossed = previous_gap / max(previous_gap - gap, 1e-8);
                float2 hit_at = lerp(at, exit_at, crossed);
                float2 hit_ndc = ndc_of(hit_at, size);
                float border = saturate((1.0 - max(abs(hit_ndc.x), abs(hit_ndc.y))) / BORDER_FADE);
                float distance = saturate((1.0 - exit_travel / travel) / DISTANCE_FADE);
                float bound = saturate(1.0 - behind / thickness);
                return saturate(1.0 - facing * border * distance * bound);
            }
        }
        previous_gap = gap;
        entry_z = exit_z;
        at_travel = exit_travel;
        if (at_travel >= travel)
        {
            break;
        }
    }
    return LIT;
}
