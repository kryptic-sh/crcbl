// crcbl-targets: spirv, wgsl, msl, dxil

// The infinite ground grid: one full-screen triangle that ray-casts the plane
// `y = 0` and draws the cell lines it crosses, blended over the scene.
//
// This is the technique Blender's overlay grid, Godot 4's editor grid and
// Unity's use, and it is a *screen-space* pass rather than geometry on purpose.
// A grid drawn as a mesh has to choose an extent (so it ends somewhere the
// camera can fly past), a tessellation (so its lines are quads with a width in
// world units), and a density LOD (so the lines do not converge into a solid
// sheet as the camera pulls back). This file has none of those: the ray hits a
// plane of infinite extent, and every line is exactly `params.z` **pixels**
// wide at any zoom because its coverage comes from `fwidth` on the world-space
// coordinate rather than from geometry.
//
// # The ray
//
// Both ends of it come out of one matrix. `inv_view_proj` takes a clip-space
// point back to world space, so unprojecting this pixel's NDC at two depths
// gives two points on its view ray — and their difference is its direction,
// with no camera position, no view matrix and no basis vectors to keep in step
// with the projection.
//
// The two depths are [`NEAR_DEPTH`] and [`MID_DEPTH`], and both are chosen to
// keep the homogeneous divide well away from zero under this engine's
// **reversed-Z, infinite far plane** projection. That projection sends the far
// plane to depth `0` with `w = 0` — so depth `0` is the one value that must not
// be unprojected, and `1.0` (the near plane) and `0.5` (twice the near
// distance) are both finite. An orthographic camera has `w = 1` throughout and
// is unaffected.
//
// # The plane, and the two discards
//
// The engine is right-handed with **+Y up and −Z forward** — see
// `crcbl_greybox::primitives`, whose every primitive stands on `y = 0`, and
// `crcbl_render::camera`'s view matrix — so the ground plane is `y = 0` and the ray
// parameter is `t = -origin.y / direction.y`. A direction with no `y` component
// is a ray parallel to the plane, and a negative `t` is a hit behind the
// camera; the fragment is dropped in both cases. It is dropped by a **flag read
// at the end** rather than by an early `discard`, because `fwidth` below is a
// quad-wide operation and a neighbour that left the quad early is a neighbour
// whose derivative is undefined.
//
// # Two scales
//
// `scale_coverage` is evaluated twice — once at the fine spacing and once at
// `params.y` times it — and the coarse result composites over the fine one. Two
// scales rather than a continuous LOD blend because the pair is what reads as a
// grid: the fine lines say where the cells are and the coarse ones say where
// the tens are, and each carries its own colour and opacity in the block.
//
// # The output is premultiplied
//
// `crcbl_hal::BlendState::premultiplied` — `src + dst * (1 - a)`. The two
// scales are composited here rather than by two draws, so the fragment already
// holds `colour * alpha` for the pair and there is nothing for the blender to
// multiply again.

/// Everything the pass needs. `crcbl_render::grid::GridParams` is the Rust
/// mirror, and it is what writes the buffer.
struct GridParams
{
    /// Clip → world, for the two unprojections that make the view ray.
    float4x4 inv_view_proj;
    /// World → clip, for the one projection that turns the plane hit back into
    /// the `SV_Depth` this pass is tested on. It is the inverse of the matrix
    /// above and is passed rather than derived, because a fragment shader has
    /// no way to invert a matrix.
    float4x4 view_proj;
    /// `x` the fine cell size in world units, `y` how many fine cells make a
    /// coarse one, `z` the line width in **pixels**, `w` the distance in world
    /// units at which the grid has faded to nothing.
    float4 params;
    /// The fine lines' colour, with their peak opacity in `a`.
    float4 fine_color;
    /// The coarse lines' colour, with their peak opacity in `a`. Composited
    /// over the fine lines.
    float4 coarse_color;
};

[[vk::binding(0, 0)]]
ConstantBuffer<GridParams> grid;

/// The depth the ray's origin is unprojected at: the **near plane** under
/// reversed-Z, where `crcbl_hal::depth`'s `NEAR` is 1.
static const float NEAR_DEPTH = 1.0;

/// The depth the ray's second point is unprojected at.
///
/// Half way in reversed-Z depth is twice the near distance in view space, which
/// is close enough to the camera that the subtraction below keeps its
/// significant bits and far enough from depth `0` that `w` is nowhere near
/// zero. Nothing about the grid depends on *which* second point is picked — the
/// direction is normalised — only that it is a different one.
static const float MID_DEPTH = 0.5;

/// The floor put under `|w|` before the homogeneous divide.
///
/// Unreachable for the two depths above under any projection this engine
/// builds; it is here so that a caller who writes a degenerate matrix into the
/// block gets a black frame rather than an infinity that poisons the `fwidth`
/// of every neighbouring pixel.
static const float W_FLOOR = 1e-6;

/// How nearly parallel to the ground a ray may be before it is treated as
/// missing the plane altogether.
///
/// The direction is a unit vector, so this is the sine of the angle to the
/// ground: about a five-thousandth of a degree.
static const float PARALLEL_EPSILON = 1e-7;

/// The floor put under a `fwidth` before it is divided by.
///
/// A derivative of exactly zero is what a completely axis-aligned top-down view
/// produces on one of the two axes, and the division below would otherwise
/// return an infinity that `saturate` maps to a full-coverage line across the
/// whole frame.
static const float DERIVATIVE_FLOOR = 1e-8;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The colour this pass blends, and the depth it is tested at.
///
/// `SV_Depth` rather than the vertex stage's constant depth, because the
/// triangle is a screen-space stand-in for a plane in the world: the fragment
/// has to be tested at the depth of the point it actually hit, or the grid
/// would be uniformly in front of or behind everything the scene drew.
struct GridOutput
{
    float4 color : SV_Target;
    float depth : SV_Depth;
};

/// The homogeneous divide, with [`W_FLOOR`] under the divisor.
float3 unproject(float4 homogeneous)
{
    float w = abs(homogeneous.w) < W_FLOOR ? W_FLOOR : homogeneous.w;
    return homogeneous.xyz / w;
}

/// How much of this scale survives once its lines start to merge, in `0..=1`.
///
/// `derivative` is the cell size a pixel spans, so a line `width` pixels across
/// covers `width * derivative` of every cell — and once that reaches one, the
/// lines have joined into a sheet. The scale is faded out over the upper half
/// of that range, so it is gone before it can become the aliased grey mush a
/// grid without a density term turns into at the horizon.
///
/// `crcbl_render::grid::resolvable` is the Rust mirror.
float resolvable(float derivative, float width)
{
    return saturate(2.0 * (1.0 - width * derivative));
}

/// Coverage of the nearest line along one axis, in `0..=1`.
///
/// `cell` is the world coordinate in cells and `derivative` is `fwidth` of it —
/// cells per pixel — so their quotient is a distance in **pixels**, which is
/// what makes the line the same width at any zoom.
///
/// # It is the exact box filter, not a ramp
///
/// `width * 0.5 + 0.5 - distance` is how much of a `width`-wide band centred on
/// the line falls inside a one-pixel box `distance` pixels away from it, and it
/// is worth the extra term over a ramp from the line to half a width out: a
/// ramp gives a hairline landing exactly on a pixel *boundary* zero coverage on
/// both sides, so a one-pixel grid drops whole lines depending on where the
/// camera happens to sit. This form gives that line a half on each side, and
/// puts the same total ink on screen wherever it lands.
///
/// `crcbl_render::grid::line_coverage` is the Rust mirror.
float line_coverage(float cell, float derivative, float width)
{
    // `frac` is `x - floor(x)` on every target, so this is the distance to the
    // nearest integer cell boundary in `0..=0.5` for negative coordinates too.
    float to_line = abs(frac(cell - 0.5) - 0.5) / max(derivative, DERIVATIVE_FLOOR);
    return saturate(width * 0.5 + 0.5 - to_line);
}

/// Coverage of one grid scale at `plane`, in `0..=1`.
///
/// The two axes are combined with `max` rather than added: a pixel on a
/// crossing is on one line, not on two, and adding would draw a bright dot at
/// every intersection.
///
/// `crcbl_render::grid::scale_coverage` is the Rust mirror.
float scale_coverage(float2 plane, float spacing, float width)
{
    float2 cell = plane / spacing;
    float2 derivative = fwidth(cell);
    float coverage = max(
        line_coverage(cell.x, derivative.x, width),
        line_coverage(cell.y, derivative.y, width));
    return coverage * resolvable(max(derivative.x, derivative.y), width);
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
GridOutput fragmentMain(FullscreenOutput input)
{
    // The vertex stage wrote `position` from `uv` with `w = 1`, so this
    // recovers that pixel's NDC exactly rather than approximately — there is no
    // perspective divide between the two to disagree about. Reading
    // `SV_Position` instead would need the target's extent, which this pass
    // binds nothing it could ask for.
    float2 ndc = input.uv * float2(2.0, -2.0) + float2(-1.0, 1.0);

    float3 origin = unproject(mul(grid.inv_view_proj, float4(ndc, NEAR_DEPTH, 1.0)));
    float3 toward = unproject(mul(grid.inv_view_proj, float4(ndc, MID_DEPTH, 1.0)));
    float3 direction = normalize(toward - origin);

    // A ray parallel to the ground never reaches it. The denominator is floored
    // rather than branched on so that `t` stays finite: an infinity here would
    // reach `fwidth` below, where it is not this pixel's problem alone.
    bool crosses = abs(direction.y) > PARALLEL_EPSILON;
    float denominator = crosses ? direction.y : PARALLEL_EPSILON;
    float t = -origin.y / denominator;

    float fade_distance = grid.params.w;
    // Behind the camera, or past the fade — both are "no grid here", and both
    // are recorded rather than acted on so that every lane of the quad reaches
    // the `fwidth` below. Clamping `t` keeps the hit finite for the lanes that
    // will be dropped, which is what keeps their derivative from poisoning a
    // neighbour that will not be.
    bool visible = crosses && t > 0.0 && t < fade_distance;
    float3 hit = origin + direction * clamp(t, 0.0, fade_distance);

    float spacing = grid.params.x;
    float multiple = grid.params.y;
    float width = grid.params.z;
    float fine = scale_coverage(hit.xz, spacing, width);
    float coarse = scale_coverage(hit.xz, spacing * multiple, width);
    float fade = saturate(1.0 - t / fade_distance);

    // Premultiplied `over`, coarse on top of fine. Two scales composited here
    // rather than by two draws, so the blender sees one already-multiplied
    // fragment.
    float coarse_alpha = grid.coarse_color.a * coarse * fade;
    float fine_alpha = grid.fine_color.a * fine * fade * (1.0 - coarse_alpha);
    float alpha = coarse_alpha + fine_alpha;
    float3 color = grid.coarse_color.rgb * coarse_alpha + grid.fine_color.rgb * fine_alpha;

    float4 clip = mul(grid.view_proj, float4(hit, 1.0));
    // Behind the eye there is no depth to write. Unreachable for a `t` the test
    // above let through, and the floor that keeps `SV_Depth` finite regardless.
    visible = visible && clip.w > 0.0 && alpha > 0.0;
    if (!visible)
    {
        discard;
    }

    GridOutput output;
    output.color = float4(color, alpha);
    // `saturate` because a depth outside `0..=1` is undefined rather than
    // clamped, and reversed-Z puts the horizon arbitrarily close to 0.
    output.depth = saturate(clip.z / clip.w);
    return output;
}
