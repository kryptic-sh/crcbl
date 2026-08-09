// crcbl-targets: spirv, wgsl, msl, dxil

// UI compositing shader: renders screen-space quads with vertex colours and
// an optional glyph-atlas texture on top of the existing framebuffer.
//
// Each quad vertex carries a position in screen-space pixels, a UV coordinate
// into the glyph atlas (zero for untextured rects), and an RGBA colour. The
// vertex stage converts to NDC by dividing through the viewport size in
// `constants`; the fragment stage composites with alpha blending.
//
// Architecture: one draw call per frame (all UI geometry merged into one
// vertex/index buffer). The glyph atlas is a single-channel R8 texture; text
// quads sample it and multiply the alpha through while keeping the vertex
// colour for the RGB channels.
//
// # One artifact, no tier split
//
// `constants` is a **bound uniform buffer on every target**, which is the
// choice `sprite.slang`'s header argues for. It was a `[[vk::push_constant]]`
// block until 2026-08, and that cost a whole second source file: WebGPU has no
// push constants, Slang's WGSL target drops the attribute without allocating a
// binding — so `wgsl/ui.wgsl` declared a `var<uniform>` with no
// `@group`/`@binding`, which naga rejects — and one entry point reads either a
// push-constant block or a bound one, never both. The twin was
// `ui_tier_b.slang`, identical but for that single declaration, held in step
// with this file by a comment, and picked between by capability in
// `crcbl-render`'s UI pass.
//
// The uniform buffer costs one indirection per vertex where a push constant
// would have served, for a block read once per vertex. It buys back the whole
// permutation axis: one source, one artifact per target, one Rust code path,
// and a `.wgsl` a browser will actually accept.

/// Per-frame constants: the framebuffer size the vertex stage divides by.
///
/// A uniform buffer on every target — see the header. `viewport` is the size in
/// pixels (width, height), and the mapping it defines puts (0,0) top-left at
/// NDC (-1,1) and (w,h) bottom-right at (1,-1), matching the Y-up convention
/// from the inverted Vulkan viewport.
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
// table. The two agree only while this file declares its resources in the order
// it numbers them, so the glyph atlas, its sampler and the vertex buffer come
// first and `constants`, at binding 3, comes last. `crcbl_shaders`'
// `declaration_order` lint checks it over every source; see
// `crcbl_mtl::binding`'s "The index a binding lands on" for the rule itself.

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
