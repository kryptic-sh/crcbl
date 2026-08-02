// Sprite pass: instanced, world-space, alpha-blended quads cut out of a sheet.
//
// One instance per sprite. The quad's six corners come from `SV_VertexID` and
// exist only as arithmetic; everything that distinguishes one sprite from the
// next — where it is in the world, which rectangle of the sheet it shows, and
// what colour it is multiplied by — comes from a read-only `StructuredBuffer`
// indexed by `SV_InstanceID`. So a thousand sprites sharing a sheet are one
// `draw(0..6, 0..1000)` and one buffer, which is what
// `docs/plan/ROADMAP.md`'s S1B finding 1 asks for: both samples put their
// world through the UI pass because `ForwardRenderer` draws exactly one
// instance, and this is the path that stops being true.
//
// # One artifact, no tier split
//
// `ui.slang` needed a `ui_tier_b.slang` twin because it delivers its constants
// through `[[vk::push_constant]]`, which WebGPU does not have — and an entry
// point either reads a push-constant block or a bound one, so the tier became a
// whole second source file that has to be kept in step by hand.
//
// This shader does not repeat that. Its constants are a **bound uniform buffer
// on every tier**, so there is one source, one `.spv`, one `.wgsl` and one Rust
// code path, and `crcbl_render::ui_pass::ConstantDelivery` has no counterpart
// here. The cost is one indirection on Tier A that a push constant would have
// avoided, for a block that is read once per vertex; the saving is the entire
// permutation axis.
//
// # Matrix layout
//
// As `mesh.slang`: a `float4x4` in a constant buffer comes out of `slangc`
// decorated `RowMajor` with `MatrixStride 16`, and `mul(m, v)` comes out as
// `OpVectorTimesMatrix`, which together put the HLSL matrix's column 0 in the
// first sixteen bytes — exactly `glam::Mat4::to_cols_array`. No transpose on
// the way in, and no `-matrix-layout-*` flag on the compile.
//
// # Two descriptor sets, because they change at different rates
//
// Set 0 is the **frame**: the constants and the instance buffer, both rotating
// once per frame in flight. Set 1 is the **sheet**: an image view and a
// sampler, created once when a sheet is registered and never touched again.
// Folding the sheet into set 0 would mean rebuilding every registered sheet's
// bind group whenever the instance ring grew; folding the frame into set 1
// would mean the same in the other direction. Batching rebinds only set 1.

/// Per-frame constants. A uniform buffer on every tier — see the header.
struct SpriteConstants
{
    /// World → clip. Orthographic in practice, but nothing here knows that:
    /// `crcbl_render::camera::Projection` decides, exactly as for `mesh.slang`.
    float4x4 view_proj;
    /// The target's size in pixels. **Unused by this shader**, and deliberately
    /// present anyway: slice 4's pixel snapping needs the fragment-per-texel
    /// ratio, and adding a field to a uniform block later is a change to every
    /// consumer of the layout. Written, padded, and read by nothing yet.
    float2 viewport;
    /// Pads the block to a multiple of 16 bytes, which `std140` requires of a
    /// uniform buffer's size and which naga enforces where a driver would not.
    float2 pad;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SpriteConstants> constants;

/// One sprite, matching `crcbl_render::sprite_pass::SpriteInstance`.
///
/// Three `float4`s — 48 bytes, no padding on any layout rule — for the same
/// reason `MeshVertex` is three: a `float3` or a `float2` pair in a structured
/// buffer acquires alignment padding the CPU side then has to reproduce
/// exactly, and a layout that cannot drift is worth four wasted bytes.
struct SpriteInstance
{
    /// World-space rectangle: `xy` is the **minimum** corner and `zw` the size.
    /// Y is up, as everywhere above `crcbl_render::camera`.
    float4 rect;
    /// Normalised sheet UVs: `xy` is the **top-left** corner of the frame and
    /// `zw` the bottom-right, in image order, which is the order
    /// `crcbl_sprite::Rect` is written in.
    float4 uv;
    /// Straight-alpha RGBA multiplied into the sampled texel.
    float4 tint;
};

[[vk::binding(1, 0)]]
StructuredBuffer<SpriteInstance> sprites;

/// The sheet: `Rgba8UnormSrgb` or `Rgba8Unorm`, straight (not premultiplied)
/// alpha.
[[vk::binding(0, 1)]]
Texture2D<float4> sheet;

/// Nearest or linear, chosen per sheet from its `SampleMode` when its bind
/// group is built. Nothing in this file can tell which it got.
[[vk::binding(1, 1)]]
SamplerState sheetSampler;

/// The unit quad, as two counter-clockwise triangles.
///
/// Six vertices rather than four plus an index buffer. The four-vertex form
/// would save two vertex invocations of about eight instructions each per
/// sprite, and would cost an index buffer: a second per-frame allocation, a
/// second thing to grow, a `bind_index_buffer` per pass and an `IndexFormat` to
/// agree on. At one triangle pair per instance the post-transform cache saves
/// nothing worth that, so the quad is arithmetic and the pass owns exactly one
/// buffer that changes per frame.
///
/// Counter-clockwise as seen with Y up, so this survives back-face culling
/// being switched on later; the pass itself culls nothing.
static const float2 CORNERS[6] = {
    float2(0.0, 0.0),
    float2(1.0, 0.0),
    float2(0.0, 1.0),
    float2(0.0, 1.0),
    float2(1.0, 0.0),
    float2(1.0, 1.0),
};

struct SpriteVarying
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
    float4 tint : COLOR0;
};

[shader("vertex")]
SpriteVarying vertexMain(uint vertex: SV_VertexID, uint instance: SV_InstanceID)
{
    SpriteInstance s = sprites[instance];
    float2 corner = CORNERS[vertex];

    // The quad in world units: minimum corner plus the corner's fraction of the
    // size. z is 0 — sprites composite in submission order with no depth test,
    // and layer ordering is the caller's, not this shader's.
    float2 world = s.rect.xy + corner * s.rect.zw;

    // **The UV's Y runs the other way from the world's.** `corner.y == 0` is the
    // world-space *bottom* of the quad, and the bottom of the quad shows the
    // *bottom* of the frame — which in image order is `uv.w`, the larger v. Take
    // the lerp the obvious way round and every sprite renders upside down while
    // looking entirely plausible on a symmetric test image.
    float2 uv = float2(
        lerp(s.uv.x, s.uv.z, corner.x),
        lerp(s.uv.w, s.uv.y, corner.y)
    );

    SpriteVarying output;
    output.position = mul(constants.view_proj, float4(world, 0.0, 1.0));
    output.uv = uv;
    output.tint = s.tint;
    return output;
}

[shader("fragment")]
float4 fragmentMain(SpriteVarying input) : SV_Target
{
    // Sampled unconditionally, and there is deliberately no branch anywhere in
    // this stage. WGSL requires `textureSample` to be reached from uniform
    // control flow; Dawn enforces that where naga does not, and one module it
    // rejects invalidates the pipeline, the command buffer and therefore the
    // whole frame. That is what left a browser drawing a black canvas while the
    // game ran normally — see `ui.slang`'s fragment stage, which is the same
    // bug written down after the fact. A sprite has nothing to branch on, so
    // keeping it that way costs nothing.
    float4 texel = sheet.Sample(sheetSampler, input.uv);

    // Straight alpha in, straight alpha out: the colour is *not* premultiplied
    // here, because the pipeline blends with `src * a + dst * (1 - a)` and doing
    // the multiply in both places darkens every edge pixel.
    return texel * input.tint;
}
