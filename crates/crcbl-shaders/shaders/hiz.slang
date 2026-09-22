// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/18-render-features.md`'s Hi-Z pyramid: one reduction step of the
// depth prepass, and the structure `ssr.slang`'s march climbs instead of
// stepping at a fixed stride.
//
// # What a level means
//
// Each texel of the destination is the **nearest surface** anywhere in the 2×2
// block of the source it covers. Under reversed-Z nearer is larger, so that is
// a `max`, and the far plane's `0.0` — `crcbl_hal::depth::CLEAR`, the value a
// pixel the geometry never covered holds — loses every comparison it is in,
// which is exactly right: a block that is empty everywhere but one texel takes
// that texel's depth, and a block that is empty everywhere stays empty.
//
// That is the whole property the march needs. A ray whose own depth is nearer
// than a cell's value is in front of everything in that cell, so it can cross
// the entire cell in one step; a ray that is not has to look inside. Level 0 of
// the pyramid is the prepass itself, at full resolution, so the finest test the
// march ever makes is against a real depth sample rather than against a
// reduction of one.
//
// # It writes depth, not colour
//
// The destination is a `D32Float` image attached as this pass's depth target and
// written through `SV_Depth`, with the comparison set to always and writes on —
// see `crcbl_render::hiz`. The alternative was an `R32Float` colour target, and
// it is worse in two ways that both come from the same place: `R32Float` is
// unfilterable in WebGPU, so binding it would need a third
// `crcbl_hal::SampleType` and a wire code for it across four backends, and the
// pyramid would then be a different texture type from the prepass at level 0 —
// so `ssr.slang` would carry two spellings of "read a depth" and a branch
// choosing between them. A depth image is the same `DepthTexture2D` the march
// already reads, at the same precision, with no seam change at all.
//
// # No sampler, and no filtered read anywhere
//
// Every fetch is a `Load` at an integer texel, on `ssr.slang`'s terms exactly:
// the value being reduced decides whether a ray skips a region of the screen,
// and a bilinear tap would put a texel-centre computation four rasterisers
// perform independently in front of that decision. `max` of integer fetches is
// the same number on all four.
//
// # Odd extents are covered rather than floored past
//
// A level's extent is the floor of half the level above it — `crcbl_render::hiz`
// halves the same way `crcbl_render::bloom` does, and for the same reason. That
// leaves an odd source with a last row or column no destination texel's 2×2
// block reaches, and dropping it would make the pyramid claim a strip of the
// screen is empty when it is not. So a destination texel whose block sits at the
// end of an odd axis takes a third tap along it, which is conservative in the
// only direction that matters: a cell that reports a nearer surface than it has
// costs the march a level of skipping, and one that reports a farther surface
// costs it the reflection.

// D3D12 takes each resource's register from its binding. A declaration's
// `D3D12_REGISTER` suffix is `register(<class><binding>, space<set>)`, the
// numbers its `[[vk::binding(binding, set)]]` names, and
// `crcbl_shaders::declaration_order` checks that the two agree. HLSL only,
// because Slang's Metal target would take its argument-table indices from a
// `register` as well.
#if defined(CRCBL_TARGET_HLSL)
#define D3D12_REGISTER(...) : register(__VA_ARGS__)
#else
#define D3D12_REGISTER(...)
#endif

/// The level being reduced: the depth prepass for level 1, and the level above
/// for every level after it.
///
/// `DepthTexture2D` for the reason `ssao.slang` gives under the same
/// declaration — it is the only spelling that lowers to a `texture_depth_2d` a
/// `D32Float` view can fill through the seam's `SampleType::Depth`.
[[vk::binding(0, 0)]]
DepthTexture2D source D3D12_REGISTER(t0, space0);

struct FullscreenOutput
{
    float4 position : SV_Position;
};

/// The destination texel's value, written to the depth attachment rather than
/// to a colour one.
///
/// A struct rather than a semantic on the return type: Slang emits a bare
/// `float fragmentMain(...) : SV_Depth` as `@location(0)` in WGSL and
/// `[[color(0)]]` in MSL — a colour output of one channel, which is a pass that
/// writes nothing to the image it claims to build.
struct HizOutput
{
    float depth : SV_Depth;
};

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // `tonemap.slang`'s triangle: one oversized primitive covering the viewport,
    // with no interpolant at all — this pass addresses its destination by
    // `SV_Position` and reads its source by integer texel.
    float2 uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

/// The two reductions' shared walk: `a` and `b` combined the way the pyramid
/// being built wants — the nearer of the two for `fragmentMain`'s pyramid, the
/// farther for `farthestMain`'s.
float combine(float a, float b, bool farthest)
{
    return farthest ? min(a, b) : max(a, b);
}

/// The destination texel's value: every tap of the 2×2 block it covers, and the
/// odd axis's leftover, combined by `combine`.
float reduce(FullscreenOutput input, bool farthest)
{
    uint width;
    uint height;
    // Read off the image rather than passed in a block: this pass renders at the
    // destination's own size and reads the source's, and a uniform carrying
    // either is a second copy of a number the images already carry.
    source.GetDimensions(width, height);
    int2 extent = int2(int(width), int(height));
    int2 destination = int2(input.position.xy);
    int2 base = destination * 2;

    // The 2×2 block, clamped so the last destination texel of an axis whose
    // source extent is odd does not read past the edge before the third tap
    // below covers what it left.
    float value = source.Load(int3(clamp(base, int2(0, 0), extent - int2(1, 1)), 0));
    value = combine(value,
        source.Load(int3(clamp(base + int2(1, 0), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
    value = combine(value,
        source.Load(int3(clamp(base + int2(0, 1), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
    value = combine(value,
        source.Load(int3(clamp(base + int2(1, 1), int2(0, 0), extent - int2(1, 1)), 0)), farthest);

    // The odd axis's leftover. `base + 2` is inside the source only for the last
    // destination texel of an axis the halving floored, so the clamp makes every
    // other texel's third tap a repeat of one it already took — `min` and `max`
    // both absorb it.
    bool odd_x = (extent.x & 1) == 1;
    bool odd_y = (extent.y & 1) == 1;
    if (odd_x)
    {
        value = combine(value,
            source.Load(int3(clamp(base + int2(2, 0), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
        value = combine(value,
            source.Load(int3(clamp(base + int2(2, 1), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
    }
    if (odd_y)
    {
        value = combine(value,
            source.Load(int3(clamp(base + int2(0, 2), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
        value = combine(value,
            source.Load(int3(clamp(base + int2(1, 2), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
    }
    if (odd_x && odd_y)
    {
        value = combine(value,
            source.Load(int3(clamp(base + int2(2, 2), int2(0, 0), extent - int2(1, 1)), 0)), farthest);
    }
    return value;
}

/// The reflection march's level: the **nearest** surface in the block.
[shader("fragment")]
HizOutput fragmentMain(FullscreenOutput input)
{
    HizOutput output;
    output.depth = reduce(input, false);
    return output;
}

/// The occlusion cull's level: the **farthest** surface in the block.
///
/// `cull.slang`'s occlusion test asks whether a box is behind *everything* drawn
/// over the pixels it covers, which is a question about the farthest depth there
/// — the conservative bound. Under reversed-Z that is a `min`, and the far
/// plane's `0.0` now wins every comparison it is in: a block with one uncovered
/// pixel hides nothing, which is exactly right, because something behind every
/// surface can still be seen through that pixel.
///
/// **Not a pyramid the march can use**, and the nearest one is not one the cull
/// can use: tested against a nearest-depth level, a box behind a thin near
/// occluder but in front of the wall behind it reads as hidden and drops out of
/// a frame it is visible in. `crcbl_render::cull`'s
/// `the_nearest_pyramid_hides_what_the_farthest_keeps` is that case.
[shader("fragment")]
HizOutput farthestMain(FullscreenOutput input)
{
    HizOutput output;
    output.depth = reduce(input, true);
    return output;
}
