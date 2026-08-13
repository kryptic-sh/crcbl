// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s screen-space ambient occlusion: the pass
// that reads the depth prepass and writes one `R8Unorm` channel the forward
// pass multiplies its **ambient** term by.
//
// # It reads depth and nothing else
//
// There is no normal attachment and no G-buffer. The prepass this samples has no
// colour target at all, so a normal buffer would mean a third geometry pipeline
// per `GeometryPath`, a new fragment entry point compiled to four targets and a
// new `VertexOutput` consumer — for a buffer exactly one pass reads. The normal
// is reconstructed here instead, from four neighbouring depths.
//
// # The technique is classic normal-oriented hemisphere SSAO
//
// Eight samples in the hemisphere around the reconstructed normal, each a single
// depth comparison against the prepass, rotated per pixel by an entry of a
// sixteen-element **constant** table. Not GTAO: its horizon integral is several
// times the work for quality nobody can resolve at the golden suite's 256×192,
// and CI's rasterisers are software. Upgrading is a change to `occlusion_at`
// alone — the pass, the resource, the binding and the test do not move — which is
// the same shape `tonemap.slang` documents for its curve.
//
// # Why the rotation is a table and never a hash
//
// This is the determinism rule, and it is why the design looks conservative.
// Each sample is a *binary* depth comparison, so one sample landing on the
// threshold resolves differently on two drivers and swings that pixel by an
// eighth — far past `crcbl_golden::Tolerance::RASTERISER`. Interleaved-gradient
// noise and `frac(sin(dot(...)))` hashes amplify float differences by
// construction, which is the opposite of what a golden needs. An integer index
// into a constant array is bit-identical by inspection.
//
// `ssao_blur.slang` is the other half of that rule and is **not optional**: its
// footprint is exactly this file's noise tile, so it both removes the banding and
// divides an isolated flipped sample by sixteen.
//
// # The full-screen triangle is `tonemap.slang`'s
//
// Same three vertices from `SV_VertexID`, same `-2.0` on Y for the same reason.
// This pass wants pixel coordinates rather than a UV, so it reads them off
// `SV_Position` in the fragment stage — which is what `mesh.slang`'s `froxel_of`
// already does with the same value.

/// Everything this pass needs that is not in a texture.
///
/// Two matrices rather than one: the reconstruction goes clip → view and each
/// sample goes view → clip again, and there is no cheaper spelling that survives
/// an orthographic camera. `crcbl_shaders::ssao::SsaoParams` is the Rust mirror
/// and `crcbl_render::forward` is what writes it.
struct SsaoParams
{
    /// Clip → view. Reversed-Z and possibly infinite, so `w` is what carries the
    /// depth and the divide below is not optional.
    float4x4 inv_proj;
    /// View → clip, for projecting a sample point back to a pixel.
    float4x4 proj;
    /// `x` is the sampling radius in **world units**, `y` the depth bias that
    /// keeps a surface from occluding itself. `zw` pad the row out to the
    /// sixteen bytes `std140` gives a uniform buffer, and nothing reads them.
    float4 params;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SsaoParams> ssao;

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
/// what the bias exists to survive — see `occlusion_at`.
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
/// indexed by `pixel.xy & 3`.
///
/// Sixteen integer directions at roughly even angles — the pairs `(±1, ±2)`,
/// `(±1, ±1)`, `(±2, ±1)` and the four axes doubled — laid out in the order a
/// 4×4 **Bayer** matrix gives, so two pixels next to each other take directions
/// from opposite sides of the circle rather than adjacent ones. Listed in tile
/// order rather than angular order for that reason: an angularly-ordered table
/// indexed this way would rotate neighbouring pixels almost identically, which is
/// banding rather than dither.
///
/// They are not unit vectors and do not need to be: `occlusion_at` runs each
/// through a Gram-Schmidt against the normal and normalises the result.
static const float2 ROTATIONS[16] = {
    float2(2.0, 0.0),   float2(-2.0, 0.0),  float2(1.0, 1.0),   float2(-1.0, -1.0),
    float2(0.0, -2.0),  float2(0.0, 2.0),   float2(1.0, -1.0),  float2(-1.0, 1.0),
    float2(1.0, 2.0),   float2(-1.0, -2.0), float2(2.0, 1.0),   float2(-2.0, -1.0),
    float2(2.0, -1.0),  float2(-2.0, 1.0),  float2(1.0, -2.0),  float2(-1.0, 2.0),
};

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

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
    float4 view = mul(ssao.inv_proj, float4(ndc, depth, 1.0));
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
/// see the header.
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

/// How much of the hemisphere over `centre` is blocked, in `0..=1`.
///
/// **Replace this function, and only this function, to upgrade the technique.**
float occlusion_at(int2 pixel, float3 centre, float3 normal, int2 extent, float2 size)
{
    float radius = ssao.params.x;
    float bias = ssao.params.y;

    // The tile's rotation, turned into a tangent frame. Gram-Schmidt against the
    // normal rather than an arbitrary basis, so the frame spins about the normal
    // from pixel to pixel and the eight samples cover different directions in
    // each pixel of the tile — which is what the blur then averages back into a
    // smooth field.
    float2 turn = ROTATIONS[(uint(pixel.y) & 3u) * 4u + (uint(pixel.x) & 3u)];
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

        float4 clip = mul(ssao.proj, float4(at, 1.0));
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

[shader("fragment")]
float fragmentMain(FullscreenOutput input) : SV_Target
{
    uint width;
    uint height;
    // Read off the image rather than passed in the block: the pass renders at the
    // depth buffer's own size by construction, and a second copy of that number
    // in a uniform is a second thing that can be stale.
    scene_depth.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    float2 size = float2(float(width), float(height));

    int2 pixel = int2(input.position.xy);
    float depth = depth_at(pixel, extent);
    // Nothing was drawn here. Fully unoccluded, and — see [`DEPTH_FAR`] — the
    // only branch that keeps the unprojection from dividing by zero.
    if (depth <= DEPTH_FAR)
    {
        return 1.0;
    }

    float3 centre = view_position(pixel, depth, size);
    float3 normal = normal_at(pixel, centre, extent, size);
    return saturate(1.0 - occlusion_at(pixel, centre, normal, extent, size));
}
