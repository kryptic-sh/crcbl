// crcbl-targets: spirv, wgsl, msl, dxil

// The occlusion chain's **cheap tier**: the same pass as `ssao.slang`, drawn
// with eight depth comparisons instead of a horizon integral.
//
// # Why there are two files
//
// `docs/plan/46-ambient-occlusion.md` decided on 2026-08-13 that classic
// normal-oriented hemisphere SSAO would stay beside GTAO rather than be
// deleted by it, on the antialiasing ladder's FXAA-under-SMAA pattern: eight
// taps and a comparison is a real budget on a software rasteriser and on a
// small device, and the two techniques share the pass, the resource, the blur
// and the tests. `ssao.slang` was rewritten in place when GTAO landed, so this
// file is that body recovered from history and brought up to today's contract —
// the half-resolution target, the four-channel output, the resolved radius —
// rather than a second technique invented for the comparison.
//
// `crcbl_render::ssao::r_ssao_technique` is the selector and
// `crcbl_render::split` is the seam a person puts the two either side of, which
// is what `docs/plan/sample/19-alcove.md`'s second milestone asks for.
//
// **A module of its own rather than a second entry point in `ssao.slang`.**
// `crcbl_shaders::Shader::entry_point` answers `None` for a stage with two
// entry points — deliberately, because picking one arbitrarily draws the wrong
// picture rather than failing — and every full-screen pipeline in the engine is
// built through it. Two fragment entries in one module is therefore a shader
// nothing can build a pipeline from.
//
// # The technique is classic normal-oriented hemisphere SSAO
//
// Eight samples in the hemisphere around the reconstructed normal, each a single
// depth comparison against the prepass, rotated per pixel by an entry of a
// sixteen-element **constant** table.
//
// **Its determinism is the weaker of the two, and that is the honest reason it
// is the cheap tier rather than an equal.** Each sample is a *binary* depth
// comparison, so one landing on the threshold resolves differently on two
// drivers and swings that pixel by an eighth — which is the argument
// `docs/plan/46-ambient-occlusion.md` used to reopen GTAO, and it has not
// changed. `ssao_blur.slang` is what contains it: its footprint is exactly this
// file's rotation tile, so it both removes the banding and divides an isolated
// flipped sample by sixteen. Nothing here may be run without it.
//
// # What it does not produce: a bent direction
//
// A hemisphere of binary comparisons measures how much of the neighbourhood is
// blocked and nothing about which way what is left of it lies. There is no arc
// to bisect, so there is no by-product to accumulate — where GTAO's bent
// direction costs a rotation per slice over horizons it already found, this
// technique would have to invent a second measurement.
//
// So every pixel writes the **zero-direction sentinel** in `gba`, which is
// `crcbl_shaders::ssao::BENT_NORMAL_NONE` after the target quantises it:
// `mesh.slang` answers that with the shading normal it already had, exactly as
// it answers a frame that ran no occlusion pass at all. A frame on this tier is
// therefore lit the way the chain lit one before the channel was widened, and
// `r_ssao_bent_normals` has nothing to switch here — the lane is read by
// `ssao.slang` alone.
//
// # Everything else is `ssao.slang`'s, and deliberately verbatim
//
// The uniform block, both bindings, the half-resolution mapping, the four
// depth helpers and the radius resolver are that file's, character for
// character where they are code: `crcbl_shaders::ssr`'s
// `the_shared_screen_space_helpers_have_not_drifted` compares the bodies as
// text across every screen-space shader, and this file is one of them. Two
// gathers that unprojected differently would be a comparison of two
// reconstructions rather than of two techniques.
//
// The full-screen triangle is `tonemap.slang`'s, for the reason that file
// gives.

/// Everything this pass needs that is not in a texture.
///
/// **`ssao.slang`'s block, member for member**, because the host binds one
/// uniform ring to whichever gather the frame selected — see
/// `crcbl_render::ssao::Ssao`. `crcbl_shaders::ssao::SsaoParams` is the Rust
/// mirror and `the_two_gathers_read_one_block` is what holds the two
/// declarations together.
struct SsaoParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` is what carries the
    /// depth and the divide below is not optional.
    float4x4 inv_proj;
    /// View → clip, for projecting a sample point back to a pixel.
    float4x4 proj;
    /// View → world, for the bent direction alone — and this file measures
    /// none, so nothing here reads it. It stays in the block because the block
    /// is shared: see this struct's own paragraph.
    float4x4 inv_view;
    /// `x` is the sampling radius in **world units**, read through
    /// [`sampling_radius`], and it is the only lane of this row this file
    /// reads. `y` is `ssao.slang`'s slice count, which a hemisphere has no
    /// planes to sweep; `z` is the intensity exponent `ssao_upsample.slang`
    /// applies at the end of the chain; `w` is the bent-direction switch, and
    /// the paragraph above is why it says nothing here.
    float4 params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsaoParams> camera;

/// The depth prepass's output, fetched by integer pixel and never filtered.
///
/// `DepthTexture2D`, not `Texture2D<float>`, and the distinction is the one this
/// engine has already been bitten by once: the two are the same HLSL declaration
/// and the same D3D12 SRV, and they are **not** the same WGSL.
/// `Texture2D<float>` lowers to `texture_2d<f32>`, which cannot be filled by a
/// `D32Float` view through a layout the seam spells `SampleType::Depth`.
/// `DepthTexture2D` lowers to `texture_depth_2d`, `depth2d<float,
/// access::sample>` in MSL and an `OpTypeImage` with `Depth = 1` in SPIR-V. See
/// `mesh.slang`'s `shadow_atlas`, which carries the same paragraph.
///
/// **No sampler.** Every read here is a `Load` at an integer pixel, so there is
/// no UV, no filtering and no texel-centre arithmetic for four rasterisers to
/// disagree about.
[[vk::binding(1, 0)]]
DepthTexture2D scene_depth;

/// The reversed-Z far plane: `crcbl_hal::depth::CLEAR`.
///
/// **The far plane has no surface**, and this is the value that says so. The
/// unprojection below divides by `clip.w`, which an infinite reversed-Z
/// projection takes to zero exactly here — so a pixel the geometry never covered
/// has to leave early rather than reconstruct a position from a division by
/// nothing.
///
/// `crcbl_shaders::ssao::DEPTH_FAR` is the Rust mirror, and
/// `crcbl_render::forward` is where it is asserted equal to the seam's own
/// constant.
static const float DEPTH_FAR = 0.0;

/// Samples taken per pixel — see the header on why eight and not a horizon
/// sweep.
static const uint SAMPLE_COUNT = 8;

/// The hemisphere kernel, in the tangent frame the normal defines: `+Z` is the
/// surface normal.
///
/// One vector per octant of the `xy` plane — `+X`, `-X`, `+Y`, `-Y` and the four
/// diagonals — at rising elevations, so the set covers the hemisphere without
/// two samples ever asking about the same direction. **Every component is a
/// multiple of an eighth**, which is exact in `f32` on every target, and no
/// trigonometry appears anywhere: `sinf` and `cosf` are the platform's and differ
/// in the last place between them, which is the same reason
/// `crcbl_shaders::mesh::OPEN_BOX_FACES` has none either.
///
/// Every vector is close to the unit sphere, and the **lateral** reach is what
/// that buys. A kernel hugging the normal answers "is there a ceiling over this
/// pixel", which almost nothing in a room is; a wall standing beside a floor is
/// occlusion that only a sample reaching sideways can find, and this set reaches
/// up to seven eighths of the radius across the surface. The low elevations are
/// what the bias exists to survive — see [`DEPTH_BIAS_RADII`].
static const float3 KERNEL[SAMPLE_COUNT] = {
    float3(0.875, 0.000, 0.250),
    float3(-0.750, 0.000, 0.375),
    float3(0.000, 0.750, 0.250),
    float3(0.000, -0.625, 0.500),
    float3(0.500, 0.500, 0.375),
    float3(-0.500, 0.500, 0.625),
    float3(0.375, -0.375, 0.750),
    float3(-0.250, -0.250, 0.875),
};

/// The rotation tile: one tangent-plane direction per pixel of a 4×4 block,
/// indexed by the **pass's own** pixel — see [`fragmentMain`], which is why the
/// tile arrives at [`occlusion_at`] as an argument rather than being derived
/// from the texel this invocation marches at.
///
/// Sixteen integer directions at roughly even angles — the pairs `(±1, ±2)`,
/// `(±1, ±1)`, `(±2, ±1)` and the four axes doubled — laid out in the order a
/// 4×4 **Bayer** matrix gives, so two pixels next to each other take directions
/// from opposite sides of the circle rather than adjacent ones. Listed in tile
/// order rather than angular order for that reason: an angularly-ordered table
/// indexed this way would rotate neighbouring pixels almost identically, which is
/// banding rather than dither.
///
/// **The same sixteen numbers `ssao.slang` marches along**, read for a
/// tangent-frame spin here and for a slice direction there. They are not unit
/// vectors and do not need to be: [`occlusion_at`] runs each through a
/// Gram-Schmidt against the normal and normalises the result.
static const float2 ROTATIONS[16] = {
    float2(2.0, 0.0),   float2(-2.0, 0.0),  float2(1.0, 1.0),   float2(-1.0, -1.0),
    float2(0.0, -2.0),  float2(0.0, 2.0),   float2(1.0, -1.0),  float2(-1.0, 1.0),
    float2(1.0, 2.0),   float2(-1.0, -2.0), float2(2.0, 1.0),   float2(-2.0, -1.0),
    float2(2.0, -1.0),  float2(-2.0, 1.0),  float2(1.0, -2.0),  float2(-1.0, 2.0),
};

/// How far in front of a surface a sample must sit before it may occlude it, as
/// a share of [`sampling_radius`].
///
/// **A threshold comparison needs one and a horizon integral does not**, which
/// is the whole of why this constant exists here and nowhere else in the chain.
/// A flat surface samples its own plane, and reconstructing that plane from a
/// quantised depth buffer puts about half the samples marginally in front of it
/// — uniform grey haze over every flat wall in the frame rather than occlusion.
/// `ssao.slang` says on its own `SsaoParams::params` why the integral that
/// replaced this technique has no such threshold to sit on, and the bias it
/// deleted is the one this file needs back.
///
/// **Taken as a share of the disc rather than as a distance**, which is
/// `ssao_blur.slang`'s `DEPTH_TOLERANCE_RADII` pattern and for its reason: the
/// radius is a console variable now, and a fixed view-space bias would be four
/// per cent of the neighbourhood at one end of that variable's range and half a
/// per cent at the other. At the radius the chain ships it is the 0.02
/// view-space units `crcbl_render::forward`'s `SSAO_BIAS` held while this
/// technique was the one that shipped.
static const float DEPTH_BIAS_RADII = 0.04;

/// The sampling radius, in world units, a frame that asks for nothing falls to.
///
/// `ssao.slang`'s constant and its argument: a producer that never writes
/// `camera.params.x` leaves a zero there, a zero disc occludes nothing, and
/// [`sampling_radius`] answers it with this rather than with [`RADIUS_MIN`] —
/// the floor is a radius somebody chose and an unwritten block is nobody
/// choosing. `crcbl_shaders::ssao`'s `the_occlusion_chain_resolves_one_radius`
/// holds every copy in the chain, this one among them.
static const float RADIUS_DEFAULT = 0.5;

/// The shortest sampling radius this pass will honour, in world units.
///
/// An eighth of [`RADIUS_DEFAULT`], and `ssao.slang`'s constant — the two
/// gathers answer one console variable, so a technique with a range of its own
/// would be a knob that means something different depending on which side of
/// the seam it is read on.
static const float RADIUS_MIN = 0.0625;

/// The longest sampling radius this pass will honour, in world units.
///
/// Eight times [`RADIUS_DEFAULT`], and [`RADIUS_MIN`]'s paragraph is this one's.
static const float RADIUS_MAX = 4.0;

/// The factor the occlusion chain's gather extent is divided by, on each axis.
///
/// **Occlusion is low-frequency, so it is not gathered per pixel.** This pass
/// and `ssao_blur.slang` run over an image this many times smaller on each side
/// — a quarter of the invocations — and `ssao_upsample.slang` reconstructs the
/// full-resolution channel the forward pass binds. `ssao.slang`'s header says
/// what that costs and what it does not.
///
/// `crcbl_shaders::ssao::RESOLUTION_DIVISOR` is the Rust mirror and
/// `crcbl_render::ssao::half_extent` is what sizes the images with it.
static const int RESOLUTION_DIVISOR = 2;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The full-resolution texel a pixel of this pass stands for.
///
/// The **top-left** of the block rather than its centre, because a block of an
/// even side has no centre texel and every fetch in this pair is a `Load`: the
/// alternative is a filtered value four rasterisers would each round their own
/// way, which is the objection `ssao.slang` makes to everything else it
/// refuses, and this file is that file's mapping.
///
/// Every pass of the chain maps the same way, which is what lets
/// `ssao_blur.slang` weight a tap by the depth this file actually gathered it
/// at, and `ssao_upsample.slang` know where each half-resolution sample sits.
/// `crcbl_shaders::ssr`'s drift guard holds those copies together.
int2 full_res_pixel(int2 pixel)
{
    return pixel * RESOLUTION_DIVISOR;
}

/// The depth prepass's value at `pixel`, clamped into the image.
///
/// Clamped rather than bounds-checked at each call site: a `Load` outside the
/// image reads zero on some targets and is undefined on others, and zero is
/// [`DEPTH_FAR`] — so an unclamped edge tap would report "nothing there" and
/// quietly stop occluding along all four borders.
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

/// View-space position of the pixel at `pixel`, given the depth already fetched
/// for it.
///
/// The caller fetches the depth because it also has to test it against
/// [`DEPTH_FAR`], and fetching twice would be a second chance for the two to
/// disagree about which pixel is being asked about.
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

/// The surface normal at `centre`, from the **closest** of the two neighbours on
/// each axis.
///
/// The two-tap `ddx`/`ddy` reconstruction is one line shorter and straddles the
/// depth discontinuity at every silhouette, which draws a dark rim around every
/// object in the frame. This one takes four taps and keeps, per axis, whichever
/// neighbour is nearer in depth to the centre — so at a silhouette it leans on
/// the side that is still the same surface.
///
/// Escalating past this — a normal attachment, or a full G-buffer — is contained
/// to the prepass pipeline and this function, and is deliberately not done here:
/// see `ssao.slang`'s header, which is where that refusal is argued for both
/// gathers.
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

/// The disc this frame asked its horizons for, in view-space units.
///
/// `camera.params.x`, which the host writes from a console variable — see
/// `crcbl_render::ssao::r_ssao_radius`. That variable's own range is the
/// console's guard and this clamp is the one that holds if the two ever
/// disagree; the host clamps as well, so a value that never reached this
/// function is caught there.
///
/// **The unwritten block is turned away upwards, like `slice_count`'s lane and
/// unlike [`RADIUS_MIN`].** World units have no zero that means anything, so a
/// zero here is a producer that wrote no block rather than a frame asking for
/// the narrowest disc.
float sampling_radius()
{
    float asked = camera.params.x;
    if (asked <= 0.0)
    {
        return RADIUS_DEFAULT;
    }
    return clamp(asked, RADIUS_MIN, RADIUS_MAX);
}

/// How much of the hemisphere over `centre` is blocked, in `0..=1`.
///
/// **This function is the technique**, and it is the only thing in this file
/// `ssao.slang` does not also contain: that file's own header says "Replace
/// this function, and only this function, to upgrade the technique", and this
/// is what the sentence was for — the two gathers differ here and nowhere else.
///
/// Eight kernel vectors are spun into the tangent frame the reconstructed
/// normal defines, projected back to the pixels that answer for them, and the
/// prepass's own surface at each is compared against the sample point.
///
/// `pixel` is the full-resolution texel this invocation gathers at and `tile`
/// is the dither index of the pixel it was invoked for — two arguments rather
/// than one derivation, for `ssao.slang`'s reason: deriving the tile from
/// `pixel` would index [`ROTATIONS`] with [`RESOLUTION_DIVISOR`]-strided
/// coordinates, collapsing the sixteen rotations onto the quarter of them that
/// lands on an even row and column, and the blur's footprint is that tile.
float occlusion_at(int2 pixel, uint tile, float3 centre, float3 normal, int2 extent, float2 size)
{
    float radius = sampling_radius();
    // In the units the comparison below is made in: view-space distance along
    // the eye axis. See [`DEPTH_BIAS_RADII`] for why it is a share of the disc.
    float bias = radius * DEPTH_BIAS_RADII;

    // The tile's rotation, turned into a tangent frame. Gram-Schmidt against the
    // normal rather than an arbitrary basis, so the frame spins about the normal
    // from pixel to pixel and the eight samples cover different directions in
    // each pixel of the tile — which is what the blur then averages back into a
    // smooth field.
    float2 turn = ROTATIONS[tile];
    float3 seed = float3(turn, 0.0);
    float3 tangent = seed - normal * dot(seed, normal);
    // A rotation that happens to lie along the normal leaves nothing to
    // normalise. It cannot happen for a surface facing the camera and a seed in
    // the `xy` plane, and the fallback costs one `select`.
    float3 across = dot(tangent, tangent) > 1e-8 ? normalize(tangent) : float3(1.0, 0.0, 0.0);
    float3 along = cross(normal, across);

    float blocked = 0.0;
    for (uint index = 0; index < SAMPLE_COUNT; ++index)
    {
        float3 offset = KERNEL[index];
        float3 at = centre + (across * offset.x + along * offset.y + normal * offset.z) * radius;

        float4 clip = mul(camera.proj, float4(at, 1.0));
        // Behind the eye: there is no pixel to ask about.
        if (clip.w <= 0.0)
        {
            continue;
        }
        float2 ndc = clip.xy / clip.w;
        int2 tap = int2(
            int((ndc.x * 0.5 + 0.5) * size.x),
            int((0.5 - ndc.y * 0.5) * size.y));
        // Off screen, rather than clamped: a sample that left the frame has no
        // evidence either way, and clamping it would smear the border pixel's
        // depth across everything near the edge.
        if (tap.x < 0 || tap.y < 0 || tap.x >= extent.x || tap.y >= extent.y)
        {
            continue;
        }

        float depth = depth_at(tap, extent);
        if (depth <= DEPTH_FAR)
        {
            continue;
        }
        float3 surface = view_position(tap, depth, size);

        // View `z` is negative ahead of the eye, so a **larger** `z` is nearer.
        // The surface the frame drew at this pixel occludes the sample point when
        // it is nearer than the sample by more than the bias.
        if (surface.z >= at.z + bias)
        {
            // Fall off with distance, so a wall metres behind the pixel does not
            // occlude it. `radius` over the gap rather than a `step`, because the
            // gap is continuous and the comparison above is already the binary
            // part.
            blocked += saturate(radius / max(abs(centre.z - surface.z), 1e-5));
        }
    }
    return blocked / float(SAMPLE_COUNT);
}

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // `tonemap.slang`'s triangle exactly: index 0 → uv (0,0), 1 → (2,0),
    // 2 → (0,2), oversized so one primitive covers the viewport.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

/// The bent direction as the three channels beside the scalar: `xyz * 0.5 +
/// 0.5`, and a zero direction lands on `0.5` in each — the value `Rgba8Unorm`
/// quantises to `0x80`, which is the byte the renderer's 1×1 placeholder holds.
/// See the header.
float3 encode_bent(float3 direction)
{
    return direction * 0.5 + 0.5;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    // Read off the image rather than passed in the block: the depth image is the
    // one this pass reads every value from, and a second copy of its size in a
    // uniform is a second thing that can be stale. It is **not** the size of the
    // target — see [`RESOLUTION_DIVISOR`] — and nothing here wants that size: a
    // pixel's rotation comes off its own coordinate and everything else is
    // measured in the full-resolution texels below.
    scene_depth.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));

    // **Two pixel coordinates, and the difference is the whole of the halving.**
    // `target` is the pixel of this pass — the target is
    // [`RESOLUTION_DIVISOR`] times smaller on each side than the depth image
    // above — and it is what indexes [`ROTATIONS`]. `pixel` is the
    // full-resolution texel it stands for, and it is what every fetch below
    // reads.
    int2 target = int2(input.position.xy);
    int2 pixel = full_res_pixel(target);
    uint tile = (uint(target.y) & 3u) * 4u + (uint(target.x) & 3u);
    float depth = depth_at(pixel, extent);
    // Nothing was drawn here. Fully unoccluded, and — see [`DEPTH_FAR`] — the
    // only branch that keeps the unprojection from dividing by zero.
    if (depth <= DEPTH_FAR)
    {
        return float4(1.0, encode_bent(float3(0.0, 0.0, 0.0)));
    }

    float3 centre = view_position(pixel, depth, size);
    float3 normal = normal_at(pixel, centre, extent, size);
    float occlusion = occlusion_at(pixel, tile, centre, normal, extent, size);
    // The sentinel in `gba`, on every pixel and unconditionally: this technique
    // measures no direction at all. See the header.
    return float4(saturate(1.0 - occlusion), encode_bent(float3(0.0, 0.0, 0.0)));
}
