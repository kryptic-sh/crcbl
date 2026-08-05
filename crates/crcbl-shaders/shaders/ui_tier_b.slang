// UI compositing shader: renders screen-space quads with vertex colours and
// an optional glyph-atlas texture on top of the existing framebuffer.
//
// Each quad vertex carries a position in screen-space pixels, a UV coordinate
// into the glyph atlas (zero for untextured rects), and an RGBA colour. The
// vertex stage converts to NDC via an orthographic projection passed through
// push constants; the fragment stage composites with alpha blending.
//
// Architecture: one draw call per frame (all UI geometry merged into one
// vertex/index buffer). The glyph atlas is a single-channel R8 texture; text
// quads sample it and multiply the alpha through while keeping the vertex
// colour for the RGB channels.

/// Push constants: orthographic projection from screen pixels to NDC.
///
/// `viewport` is the framebuffer size in pixels (width, height). The matrix
/// maps (0,0) top-left to (-1,1) and (w,h) bottom-right to (1,-1) — matching
/// the Y-up convention from the inverted Vulkan viewport.
///
/// **Tier B variant of `ui.slang`.** Byte-identical to it except that
/// `constants` is a bound uniform buffer rather than a push constant, because
/// WebGPU has no push constants at all. Slang's WGSL target also drops the
/// `[[vk::push_constant]]` attribute without allocating a binding, so
/// `wgsl/ui.wgsl` declares a `var<uniform>` with no `@group`/`@binding` and
/// naga rejects it — this file is what a WebGPU device actually compiles.
/// `crcbl_render::ConstantDelivery` picks between the two by capability; keep
/// the two sources in step, changing `ui.slang` means changing this one too.
struct UiConstants
{
    float2 viewport;       // framebuffer size in pixels
};

/// One drawn quad vertex.
struct Vertex
{
    float2 position;   // screen-space pixels
    float2 uv;         // glyph atlas UV (zero for untextured)
    float4 color;      // RGBA
};

// **Declaration order is binding order, and it is load-bearing on Metal.**
// Slang's Metal target ignores `[[vk::binding]]` and assigns each resource an
// index in its own flat argument table in the order the declarations appear,
// while `crcbl-mtl` flattens `(set, binding)` by ascending binding number per
// table. This file used to declare `constants` first while numbering it last,
// so its MSL put `constants` at `buffer(0)` and `vertices` at `buffer(1)` — the
// reverse of what the backend binds, which made the UI pass read its vertices
// out of the viewport constants and draw nothing on macOS. Keep the two orders
// in step; see `crcbl_mtl::binding`'s "The index a binding lands on".

// The glyph atlas: R8_UNORM, so the shader sees r=alpha, g=b=a=0 or 1 depending
// on the implementation. We only read `.r`.
[[vk::binding(0, 0)]]
Texture2D<float> glyphAtlas;

[[vk::binding(1, 0)]]
SamplerState glyphSampler;

/// The merged UI geometry for this frame. Read-only storage buffer.
[[vk::binding(2, 0)]]
StructuredBuffer<Vertex> vertices;

[[vk::binding(3, 0)]]
ConstantBuffer<UiConstants> constants;

struct UiOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

[shader("vertex")]
UiOutput vertexMain(uint index: SV_VertexID)
{
    Vertex v = vertices[index];

    // Map screen-space (0,0 top-left) to NDC (-1,1 top-left) to (1,-1)
    // bottom-right, matching the engine's Y-up Vulkan coordinate system.
    float2 ndc;
    ndc.x = (v.position.x / constants.viewport.x) * 2.0 - 1.0;
    ndc.y = 1.0 - (v.position.y / constants.viewport.y) * 2.0;

    UiOutput output;
    output.position = float4(ndc, 0.0, 1.0);
    output.uv = v.uv;
    output.color = v.color;
    return output;
}

[shader("fragment")]
float4 fragmentMain(UiOutput input) : SV_Target
{
    float4 color = input.color;

    // When UV is non-zero, this is a textured glyph — modulate by the atlas.
    // Glyph quads all have UV in (0,1); rect quads are at (0,0).
    //
    // The sample sits *above* the branch on purpose. WGSL requires
    // `textureSample` to be called from uniform control flow, and `input.uv` is
    // a varying — so sampling inside the `if` is a hard error in Dawn, which
    // enforces the rule, even though naga accepts it and every Vulkan driver
    // runs it. One invalid module invalidates its pipeline, its command buffer
    // and therefore the whole frame, so this branch is why a browser drew
    // nothing at all. Sampling unconditionally costs a fetch on rect quads,
    // whose UV is (0,0), and the result is then discarded.
    float glyph = glyphAtlas.Sample(glyphSampler, input.uv);
    bool textured = input.uv.x > 0.0 || input.uv.y > 0.0;
    color.a *= textured ? glyph : 1.0;

    return color;
}
