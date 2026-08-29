// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/49-antialiasing.md`'s SMAA 1x, second of three passes: the
// blend-weight calculation. `smaa_edges.slang`'s header describes the tier and
// what each pass hands the next.
//
// This is the pass the two committed tables exist for. It reads the edge mask,
// walks along each edge to find where the pattern it belongs to starts and
// ends, and reads the coverage that pattern implies out of `crcbl_shaders`'
// **area** table — with the **search** table turning the last step of each walk
// into one fetch instead of a per-texel test. Both are transcribed from the
// reference generators and committed as bytes; `crcbl_shaders::smaa` is where
// that is argued.
//
// # How the tables are addressed here
//
// The reference's area texture is 160×560 — seven subsample slabs of orthogonal
// patterns beside five of diagonal ones — and it addresses it with
// `SMAA_AREATEX_PIXEL_SIZE` of `1/(160, 560)`, then slides down by
// `SMAA_AREATEX_SUBTEX_SIZE * offset` to reach the slab a subsample index
// wants. **Only the offset-zero slab is committed here**, because SMAA 1x is
// the only rung this engine has: `tables/smaa_area.bin` is 160×80, the pixel
// size below is `1/(AREA_WIDTH, AREA_HEIGHT)`, and there is no subtexture
// offset to apply because there is nothing to select. The `+= 0.5` that reaches
// the diagonal half survives unchanged, because the committed slab is the
// reference's own first 80 rows and the diagonal block starts at its halfway
// column exactly.
//
// The search texture is the reference's whole, cropped and flipped by its own
// generator: 64×16 bytes. `SMAA_SEARCHTEX_SIZE` (66, 33) and
// `SMAA_SEARCHTEX_PACKED_SIZE` (64, 16) below are the reference's two constants
// and both are load-bearing — the first scales the edge pair into the
// uncropped image's coordinates and the second converts that to texture
// coordinates of the cropped one. The stored byte is the step times
// `SMAA_SEARCH_STEP_SCALE`, which the `255/127` below divides back out.
//
// # A linear sampler, and the area lookup depends on it
//
// Every fetch in this pass is between texels on purpose. The edge fetches are
// the reference's pseudo-gather — one bilinear tap placed at (-0.25, -0.125)
// reads four edges at once, and the fractional value it returns is what the
// search table is indexed by. And the **area table is interpolated**: a
// distance is a `sqrt` of a pixel count, which lands between the tabulated
// texels, and a nearest sampler would quantise every blend weight to the
// nearest tabulated distance. `crcbl_render::smaa` creates that sampler and
// says so there too.
//
// # The searches run a constant number of steps
//
// The reference's searches are `while` loops bounded by a texture coordinate it
// computed ahead of them. Here they are `for` loops of `MAX_SEARCH_STEPS`
// iterations with a flag stopping the advance, which is `fxaa.slang`'s
// determinism shape and is exactly equivalent: the reference's bound is the
// start coordinate less `2 * MAX_SEARCH_STEPS` texels and the walk takes two
// texels a step. Doing it by count rather than by coordinate also removes the
// one way the two could disagree — sixteen accumulated subtractions of a texel
// size need not land on the same float as one multiplication by thirty-two.
//
// # No `#include`
//
// `bloom_composite.slang`'s rule; `smaa_edges.slang` repeats it. The parameter
// block and the fullscreen triangle below are copies.

/// Everything the three SMAA passes take from the CPU — `smaa_edges.slang`
/// declares the same block and its header says why there is one.
struct SmaaParams
{
    /// One over the extent of the image being read — `SMAA_RT_METRICS.xy`.
    float2 inv_source;
    /// That extent in texels — `SMAA_RT_METRICS.zw`.
    float2 source_size;
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

/// The edge mask `smaa_edges.slang` wrote: red west, green north.
[[vk::binding(0, 0)]]
Texture2D<float4> edges;

/// The committed area table, `Rg8Unorm`, [`AREA_WIDTH`]×[`AREA_HEIGHT`].
[[vk::binding(1, 0)]]
Texture2D<float4> area;

/// The committed search table, `R8Unorm`, 64×16.
[[vk::binding(2, 0)]]
Texture2D<float4> search;

/// A **linear**, clamp-to-edge sampler for all three — see the header, and
/// `crcbl_render::smaa`, which builds it.
[[vk::binding(3, 0)]]
SamplerState tableSampler;

[[vk::binding(4, 0)]]
ConstantBuffer<SmaaParams> params;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// `SMAA_MAX_SEARCH_STEPS` at `SMAA_PRESET_HIGH`. Each step covers two texels,
/// so this reaches thirty-two pixels along an edge in each direction.
static const int MAX_SEARCH_STEPS = 16;

/// `SMAA_MAX_SEARCH_STEPS_DIAG` at `SMAA_PRESET_HIGH`.
static const int MAX_SEARCH_STEPS_DIAG = 8;

/// `SMAA_CORNER_ROUNDING` at `SMAA_PRESET_HIGH`, as its normalised form: how
/// much of a corner's blend is kept, where `1.0` would round corners as hard as
/// any other pattern and `0.0` would leave them square.
static const float CORNER_ROUNDING_NORM = 25.0 / 100.0;

/// `SMAA_AREATEX_MAX_DISTANCE`: the orthogonal patterns' tabulated reach, and
/// the side of one pattern tile in the area table's left block.
static const float AREATEX_MAX_DISTANCE = 16.0;

/// `SMAA_AREATEX_MAX_DISTANCE_DIAG`: the same for the diagonal block.
static const float AREATEX_MAX_DISTANCE_DIAG = 20.0;

/// The area table's texel size — `1/(AREA_WIDTH, AREA_HEIGHT)`, and **not** the
/// reference's `1/(160, 560)`. The header says why: one slab is committed.
static const float2 AREATEX_PIXEL_SIZE = float2(1.0 / 160.0, 1.0 / 80.0);

/// `SMAA_SEARCHTEX_SIZE`: the uncropped search image the reference's scale and
/// bias are written in terms of.
static const float2 SEARCHTEX_SIZE = float2(66.0, 33.0);

/// `SMAA_SEARCHTEX_PACKED_SIZE`: the committed, cropped image's extent.
static const float2 SEARCHTEX_PACKED_SIZE = float2(64.0, 16.0);

/// The reciprocal of `crcbl_shaders::smaa::SEARCH_STEP_SCALE` over an `R8Unorm`
/// decode: the table stores a step of one or two as 127 or 254, which read back
/// as `127/255` and `254/255`.
static const float SEARCH_STEP_DECODE = 255.0 / 127.0;

/// One bilinear tap of the edge mask.
float4 sample_edges(float2 uv)
{
    return edges.SampleLevel(tableSampler, uv, 0.0);
}

/// The same, `offset` whole texels away.
///
/// Whole-texel arithmetic on the coordinate rather than a `SampleLevel` offset
/// argument: the two are identical for a single-mip clamped image, and the
/// coordinate form needs no compile-time-constant offset to survive four
/// translations.
float4 sample_edges_at(float2 uv, float2 offset)
{
    return edges.SampleLevel(tableSampler, uv + offset * params.inv_source, 0.0);
}

/// The reference's `SMAADecodeDiagBilinearAccess`, two components.
///
/// A bilinear fetch of the edges at a 0.25 offset returns 0.25 or 1.0 in red
/// and 0.75 or 1.0 in green; this unpacks both back to binary.
float2 decode_diag_bilinear(float2 e)
{
    e.r = e.r * abs(5.0 * e.r - 5.0 * 0.75);
    return round(e);
}

/// The same for two pairs at once — the reference's `float4` overload, which
/// unpacks the `r` and `b` lanes.
float4 decode_diag_bilinear4(float4 e)
{
    e.r = e.r * abs(5.0 * e.r - 5.0 * 0.75);
    e.b = e.b * abs(5.0 * e.b - 5.0 * 0.75);
    return round(e);
}

/// The reference's `SMAASearchDiag1`: walks `dir` until the diagonal edge runs
/// out, and answers `(steps taken, the last fetch's strength)`.
float2 search_diag1(float2 uv, float2 dir, out float2 e)
{
    float3 coord = float3(uv, -1.0);
    float weight = 1.0;
    e = float2(0.0, 0.0);
    for (int i = 0; i < MAX_SEARCH_STEPS_DIAG; ++i)
    {
        if (coord.z < float(MAX_SEARCH_STEPS_DIAG - 1) && weight > 0.9)
        {
            coord += float3(dir * params.inv_source, 1.0);
            e = sample_edges(coord.xy).rg;
            weight = dot(e, float2(0.5, 0.5));
        }
    }
    return float2(coord.z, weight);
}

/// The reference's `SMAASearchDiag2`: the same walk, fetching both edges of a
/// texel in one tap — its `@SearchDiag2Optimization`, quarter-texel offset and
/// unpack included.
float2 search_diag2(float2 uv, float2 dir, out float2 e)
{
    float3 coord = float3(uv, -1.0);
    coord.x += 0.25 * params.inv_source.x;
    float weight = 1.0;
    e = float2(0.0, 0.0);
    for (int i = 0; i < MAX_SEARCH_STEPS_DIAG; ++i)
    {
        if (coord.z < float(MAX_SEARCH_STEPS_DIAG - 1) && weight > 0.9)
        {
            coord += float3(dir * params.inv_source, 1.0);
            e = decode_diag_bilinear(sample_edges(coord.xy).rg);
            weight = dot(e, float2(0.5, 0.5));
        }
    }
    return float2(coord.z, weight);
}

/// The reference's `SMAAAreaDiag`: the coverage a diagonal line of length
/// `dist` with crossing edges `e` puts either side of this pixel.
///
/// The right half of the table, addressed in this module's own texel size — see
/// the header. There is no subtexture offset: `SMAA_AREATEX_SUBTEX_SIZE * 0`
/// for SMAA 1x, and the slab it would select is the only one committed.
float2 area_diag(float2 dist, float2 e)
{
    float2 texcoord =
        float2(AREATEX_MAX_DISTANCE_DIAG, AREATEX_MAX_DISTANCE_DIAG) * e + dist;
    texcoord = AREATEX_PIXEL_SIZE * texcoord + 0.5 * AREATEX_PIXEL_SIZE;
    texcoord.x += 0.5;
    return area.SampleLevel(tableSampler, texcoord, 0.0).rg;
}

/// The reference's `SMAACalculateDiagWeights`: the weights a diagonal pattern
/// through this pixel implies, or zero where there is none.
///
/// Diagonals take priority over the orthogonal search — a staircase running at
/// 45° is the case the orthogonal patterns cannot describe — so the caller
/// skips the horizontal and vertical work when this returns anything.
float2 calculate_diag_weights(float2 uv, float2 e)
{
    float2 weights = float2(0.0, 0.0);

    // The two ends of a `\`-running line: `d.xy` how far each reached, `d.zw`
    // whether it stopped on an edge or ran out of steps.
    float4 d;
    float2 end;
    if (e.r > 0.0)
    {
        float2 found = search_diag1(uv, float2(-1.0, 1.0), end);
        d.x = found.x;
        d.z = found.y;
        d.x += float(end.y > 0.9);
    }
    else
    {
        d.x = 0.0;
        d.z = 0.0;
    }
    float2 found_positive = search_diag1(uv, float2(1.0, -1.0), end);
    d.y = found_positive.x;
    d.w = found_positive.y;

    // `d.x + d.y + 1 > 3` in the reference's own comment: a line of at least
    // four pixels is one worth revectorising.
    if (d.x + d.y > 2.0)
    {
        float4 coords =
            float4(-d.x + 0.25, d.x, d.y, -d.y - 0.25) * float4(params.inv_source, params.inv_source)
            + float4(uv, uv);
        float4 fetched;
        fetched.xy = sample_edges_at(coords.xy, float2(-1.0, 0.0)).rg;
        fetched.zw = sample_edges_at(coords.zw, float2(1.0, 0.0)).rg;
        // The reference writes this as `c.yxwz = decode(c.xyzw)`, a swizzled
        // assignment; spelled out because a swizzle on the left of an
        // assignment is one more thing four translations have to agree about.
        float4 decoded = decode_diag_bilinear4(fetched);
        float4 c = float4(decoded.y, decoded.x, decoded.w, decoded.z);

        float2 crossing = float2(2.0, 2.0) * c.xz + c.yw;
        // A search that stopped because it ran out of steps found no end, so
        // its crossing edge is not one — the reference's `SMAAMovc` over
        // `step(0.9, d.zw)`.
        if (d.z >= 0.9)
        {
            crossing.x = 0.0;
        }
        if (d.w >= 0.9)
        {
            crossing.y = 0.0;
        }
        weights += area_diag(d.xy, crossing);
    }

    // And the `/`-running line.
    float2 found_negative = search_diag2(uv, float2(-1.0, -1.0), end);
    d.x = found_negative.x;
    d.z = found_negative.y;
    if (sample_edges_at(uv, float2(1.0, 0.0)).r > 0.0)
    {
        float2 found = search_diag2(uv, float2(1.0, 1.0), end);
        d.y = found.x;
        d.w = found.y;
        d.y += float(end.y > 0.9);
    }
    else
    {
        d.y = 0.0;
        d.w = 0.0;
    }

    if (d.x + d.y > 2.0)
    {
        float4 coords = float4(-d.x, -d.x, d.y, d.y)
                * float4(params.inv_source, params.inv_source)
            + float4(uv, uv);
        float4 c;
        c.x = sample_edges_at(coords.xy, float2(-1.0, 0.0)).g;
        c.y = sample_edges_at(coords.xy, float2(0.0, -1.0)).r;
        float2 far = sample_edges_at(coords.zw, float2(1.0, 0.0)).rg;
        // The reference's `.gr`: green into `z`, red into `w`.
        c.z = far.y;
        c.w = far.x;

        float2 crossing = float2(2.0, 2.0) * c.xz + c.yw;
        if (d.z >= 0.9)
        {
            crossing.x = 0.0;
        }
        if (d.w >= 0.9)
        {
            crossing.y = 0.0;
        }
        float2 found_area = area_diag(d.xy, crossing);
        // `.gr` again: this diagonal runs the other way, so its two sides swap.
        weights += float2(found_area.y, found_area.x);
    }

    return weights;
}

/// The reference's `SMAASearchLength`: how much further the last step of a
/// walk reached, read out of the search table.
///
/// The table is indexed by the pair of bilinear edge fetches the walk ended on,
/// which between them say which of four edges were active — that is the whole
/// trick, and `crcbl_shaders::smaa::bake_search` is where the same encoding is
/// generated. `offset` selects the left half of the table or the right.
float search_length(float2 e, float offset)
{
    // The table is flipped vertically, with the left and right cases taking
    // half of it each.
    float2 scale = SEARCHTEX_SIZE * float2(0.5, -1.0);
    float2 bias = SEARCHTEX_SIZE * float2(offset, 1.0);

    // Scale and bias to texel centres.
    scale += float2(-1.0, 1.0);
    bias += float2(0.5, -0.5);

    // Into texture coordinates of the *cropped* image, which is what is
    // committed — the reference's own note beside `SMAA_SEARCHTEX_PACKED_SIZE`.
    scale *= 1.0 / SEARCHTEX_PACKED_SIZE;
    bias *= 1.0 / SEARCHTEX_PACKED_SIZE;

    return search.SampleLevel(tableSampler, scale * e + bias, 0.0).r;
}

/// The reference's `SMAASearchXLeft`. `uv` is already offset by
/// (-0.25, -0.125) texels, which is the pseudo-gather placement that lets one
/// tap read four edges and stay tellable apart.
float search_x_left(float2 uv)
{
    float2 e = float2(0.0, 1.0);
    float2 coord = uv;
    for (int i = 0; i < MAX_SEARCH_STEPS; ++i)
    {
        // "Is there some edge not activated?" and "is there a crossing edge
        // that breaks the line?", in the reference's words.
        if (e.g > 0.8281 && e.r == 0.0)
        {
            e = sample_edges(coord).rg;
            coord -= float2(2.0, 0.0) * params.inv_source;
        }
    }
    float offset = -SEARCH_STEP_DECODE * search_length(e, 0.0) + 3.25;
    return params.inv_source.x * offset + coord.x;
}

/// The reference's `SMAASearchXRight`.
float search_x_right(float2 uv)
{
    float2 e = float2(0.0, 1.0);
    float2 coord = uv;
    for (int i = 0; i < MAX_SEARCH_STEPS; ++i)
    {
        if (e.g > 0.8281 && e.r == 0.0)
        {
            e = sample_edges(coord).rg;
            coord += float2(2.0, 0.0) * params.inv_source;
        }
    }
    float offset = -SEARCH_STEP_DECODE * search_length(e, 0.5) + 3.25;
    return -params.inv_source.x * offset + coord.x;
}

/// The reference's `SMAASearchYUp`.
float search_y_up(float2 uv)
{
    float2 e = float2(1.0, 0.0);
    float2 coord = uv;
    for (int i = 0; i < MAX_SEARCH_STEPS; ++i)
    {
        if (e.r > 0.8281 && e.g == 0.0)
        {
            e = sample_edges(coord).rg;
            coord -= float2(0.0, 2.0) * params.inv_source;
        }
    }
    float offset = -SEARCH_STEP_DECODE * search_length(float2(e.g, e.r), 0.0) + 3.25;
    return params.inv_source.y * offset + coord.y;
}

/// The reference's `SMAASearchYDown`.
float search_y_down(float2 uv)
{
    float2 e = float2(1.0, 0.0);
    float2 coord = uv;
    for (int i = 0; i < MAX_SEARCH_STEPS; ++i)
    {
        if (e.r > 0.8281 && e.g == 0.0)
        {
            e = sample_edges(coord).rg;
            coord += float2(0.0, 2.0) * params.inv_source;
        }
    }
    float offset = -SEARCH_STEP_DECODE * search_length(float2(e.g, e.r), 0.5) + 3.25;
    return -params.inv_source.y * offset + coord.y;
}

/// The reference's `SMAAArea`: the coverage either side of an orthogonal
/// pattern of length `dist`, whose ends carry the crossing edges `e1` and `e2`.
///
/// `dist` arrives as a square root, because the area table compresses the
/// distance axis quadratically — `crcbl_shaders::smaa::AREA_MAX_DISTANCE` says
/// so from the generator's side. The `round(4 * e)` is the reference's own
/// guard against a bilinear fetch's last bits moving a pattern index.
float2 area_ortho(float2 dist, float e1, float e2)
{
    float2 texcoord = float2(AREATEX_MAX_DISTANCE, AREATEX_MAX_DISTANCE)
            * round(4.0 * float2(e1, e2))
        + dist;
    texcoord = AREATEX_PIXEL_SIZE * texcoord + 0.5 * AREATEX_PIXEL_SIZE;
    return area.SampleLevel(tableSampler, texcoord, 0.0).rg;
}

/// The reference's `SMAADetectHorizontalCornerPattern`, as the factor it scales
/// the weights by rather than as an `inout`.
float2 horizontal_corner_factor(float4 texcoord, float2 d)
{
    float2 left_right = step(d.xy, d.yx);
    float2 rounding = (1.0 - CORNER_ROUNDING_NORM) * left_right;
    // Less blending for a pixel in the middle of a line.
    rounding /= left_right.x + left_right.y;

    float2 factor = float2(1.0, 1.0);
    factor.x -= rounding.x * sample_edges_at(texcoord.xy, float2(0.0, 1.0)).r;
    factor.x -= rounding.y * sample_edges_at(texcoord.zw, float2(1.0, 1.0)).r;
    factor.y -= rounding.x * sample_edges_at(texcoord.xy, float2(0.0, -2.0)).r;
    factor.y -= rounding.y * sample_edges_at(texcoord.zw, float2(1.0, -2.0)).r;
    return saturate(factor);
}

/// The reference's `SMAADetectVerticalCornerPattern`, likewise.
float2 vertical_corner_factor(float4 texcoord, float2 d)
{
    float2 left_right = step(d.xy, d.yx);
    float2 rounding = (1.0 - CORNER_ROUNDING_NORM) * left_right;
    rounding /= left_right.x + left_right.y;

    float2 factor = float2(1.0, 1.0);
    factor.x -= rounding.x * sample_edges_at(texcoord.xy, float2(1.0, 0.0)).g;
    factor.x -= rounding.y * sample_edges_at(texcoord.zw, float2(1.0, 1.0)).g;
    factor.y -= rounding.x * sample_edges_at(texcoord.xy, float2(-2.0, 0.0)).g;
    factor.y -= rounding.y * sample_edges_at(texcoord.zw, float2(-2.0, 1.0)).g;
    return saturate(factor);
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
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    float2 uv = input.uv;
    // This pixel in pixel units, which the orthogonal distances below are
    // measured against — the reference's `pixcoord`.
    float2 pixcoord = uv * params.source_size;
    float2 texel = params.inv_source;

    // The reference's `offset[0]` and `offset[1]`: the pseudo-gather placements
    // the four searches start from. Its `offset[2]` held the loop bounds and is
    // gone with them — see the header.
    float4 offset_h = float4(-0.25, -0.125, 1.25, -0.125) * float4(texel, texel) + float4(uv, uv);
    float4 offset_v = float4(-0.125, -0.25, -0.125, 1.25) * float4(texel, texel) + float4(uv, uv);

    float4 weights = float4(0.0, 0.0, 0.0, 0.0);
    float2 e = sample_edges(uv).rg;

    if (e.g > 0.0)
    {
        // A diagonal has both a north and a west edge, so looking for one on
        // the north side is enough.
        float2 diagonal = calculate_diag_weights(uv, e);
        weights.r = diagonal.x;
        weights.g = diagonal.y;

        // `weights.r == -weights.g` is the reference's spelling of "both are
        // zero", which means no diagonal was found and the orthogonal search
        // takes over.
        if (weights.r == -weights.g)
        {
            float2 d;
            float3 coords;

            coords.x = search_x_left(offset_h.xy);
            // The crossing offset the fetch below needs: a quarter texel up, so
            // one tap tells the two edges apart.
            coords.y = offset_v.y;
            d.x = coords.x;
            float e1 = sample_edges(coords.xy).r;

            coords.z = search_x_right(offset_h.zw);
            d.y = coords.z;

            // Into pixel units, which is where the pattern's length lives.
            d = abs(round(params.source_size.xx * d - pixcoord.xx));
            // The area table's distance axis is compressed quadratically.
            float2 sqrt_d = sqrt(d);

            float e2 = sample_edges_at(float2(coords.z, coords.y), float2(1.0, 0.0)).r;

            float2 found = area_ortho(sqrt_d, e1, e2);

            coords.y = uv.y;
            found *= horizontal_corner_factor(
                float4(coords.x, coords.y, coords.z, coords.y), d);
            weights.r = found.x;
            weights.g = found.y;
        }
        else
        {
            // A diagonal was found, so skip the vertical processing too.
            e.r = 0.0;
        }
    }

    if (e.r > 0.0)
    {
        float2 d;
        float3 coords;

        coords.y = search_y_up(offset_v.xy);
        coords.x = offset_h.x;
        d.x = coords.y;
        float e1 = sample_edges(coords.xy).g;

        coords.z = search_y_down(offset_v.zw);
        d.y = coords.z;

        d = abs(round(params.source_size.yy * d - pixcoord.yy));
        float2 sqrt_d = sqrt(d);

        float e2 = sample_edges_at(float2(coords.x, coords.z), float2(0.0, 1.0)).g;

        float2 found = area_ortho(sqrt_d, e1, e2);

        coords.x = uv.x;
        found *= vertical_corner_factor(float4(coords.x, coords.y, coords.x, coords.z), d);
        weights.b = found.x;
        weights.a = found.y;
    }

    return weights;
}
