// crcbl-targets: spirv, wgsl, msl, dxil

// UI compositing shader: renders screen-space quads on top of the existing
// framebuffer — solid colour, glyph-atlas text, image-atlas pictures and
// analytic rounded rectangles, all through one pipeline.
//
// Each quad vertex carries a position in screen-space pixels, a UV, an RGBA
// colour, the clip rectangle it was pushed under, and the primitive it belongs
// to with that primitive's parameters (`crcbl_ui::draw_list::Vertex2d` is the
// Rust half, lane for lane). The vertex stage converts to NDC by dividing
// through the viewport size in `constants`; the fragment stage picks what to
// draw from the primitive and composites with alpha blending.
//
// Architecture: one draw call per half of the draw list (all UI geometry merged
// into one vertex/index buffer). The glyph atlas is a single-channel R8 texture
// multiplied into the vertex colour's alpha; the image atlas is `Rgba8UnormSrgb`
// multiplied by the vertex colour. A primitive switch that is data rather than
// a pipeline is what keeps a menu frame, its labels and a rounded panel in the
// same batch.
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
//
// # No branch reaches a sample or a derivative
//
// WGSL requires `textureSample` and `fwidth` to be called from uniform control
// flow, and the primitive is a per-vertex value — so both atlases are sampled,
// and the sharp-bilinear derivative taken, for **every** fragment before
// anything is chosen, and the clip's `discard` is the fragment stage's last
// statement. Dawn enforces the rule where naga and every Vulkan driver do not,
// and one module it rejects invalidates its pipeline, its command buffer and
// therefore the whole frame: sampling inside a branch here is exactly how a
// browser once drew nothing at all.

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

/// One drawn quad vertex: `crcbl_ui::draw_list::Vertex2d`, lane for lane.
struct Vertex
{
    float2 position;   // screen-space pixels
    float2 uv;         // atlas UV; a rounded rect's offset from its centre
    float4 color;      // RGBA fill, text colour or image tint
    float4 clip;       // min.x, min.y, max.x, max.y in screen pixels
    float4 shape;      // half width, half height, border width, primitive
    float4 radii;      // top-left, top-right, bottom-right, bottom-left
    float4 border;     // a rounded rect's border colour
};

/// `crcbl_ui::draw_list::Primitive`'s lanes.
static const float PRIMITIVE_GLYPH = 1.0;
static const float PRIMITIVE_IMAGE = 2.0;
static const float PRIMITIVE_ROUNDED_RECT = 3.0;

// **Declaration order is binding order, and it is load-bearing on Metal.**
// Slang's Metal target ignores `[[vk::binding]]` and assigns each resource an
// index in its own flat argument table in the order the declarations appear,
// while `crcbl-mtl` flattens `(set, binding)` by ascending binding number per
// table. The two agree only while this file declares its resources in the order
// it numbers them, so the glyph atlas, its sampler and the vertex buffer come
// first, `constants` at binding 3 next, and the image atlas and its sampler,
// at 4 and 5, last. `crcbl_shaders`' `declaration_order` lint checks it over
// every source; see `crcbl_mtl::binding`'s "The index a binding lands on" for
// the rule itself.

// The glyph atlas: R8_UNORM, so the shader sees r=alpha, g=b=a=0 or 1 depending
// on the implementation. We only read `.r`.
[[vk::binding(0, 0)]]
Texture2D<float> glyphAtlas;

/// Nearest: the glyphs are a bitmap font drawn at whole multiples.
[[vk::binding(1, 0)]]
SamplerState glyphSampler;

/// The merged UI geometry for this frame. Read-only storage buffer.
[[vk::binding(2, 0)]]
StructuredBuffer<Vertex> vertices;

[[vk::binding(3, 0)]]
ConstantBuffer<UiConstants> constants;

/// The image atlas: `Rgba8UnormSrgb`, straight alpha — `crcbl_ui::image`.
[[vk::binding(4, 0)]]
Texture2D<float4> imageAtlas;

/// **Linear**, bent into sharp-bilinear by `sharpen` below: the same sampling
/// the sprite pass gives `SampleMode::Pixel` sheets.
[[vk::binding(5, 0)]]
SamplerState imageSampler;

struct UiOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
    /// The vertex's own screen position, interpolated, which is what the clip
    /// is tested against. Not `SV_Position`: its origin and its Y direction are
    /// each backend's, and this is the pixel space the clip was written in.
    float2 screen : TEXCOORD1;
    /// `nointerpolation` because all four are per-quad constants: every vertex
    /// of the quad writes the same value, so interpolating them is arithmetic
    /// whose answer is already known.
    nointerpolation float4 clip : TEXCOORD2;
    nointerpolation float4 shape : TEXCOORD3;
    nointerpolation float4 radii : TEXCOORD4;
    nointerpolation float4 border : TEXCOORD5;
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
    output.screen = v.position;
    output.clip = v.clip;
    output.shape = v.shape;
    output.radii = v.radii;
    output.border = v.border;
    return output;
}

/// `sprite.slang`'s `sharpen`, unchanged: bends `uv` so a linear sampler
/// behaves like nearest everywhere except a band one fragment wide at each
/// texel boundary. That file carries the derivation; the property that matters
/// here is that at a whole number of pixels per texel it collapses to nearest
/// exactly, so a menu frame drawn through this pass is the same pixels the
/// sprite pass drew it as.
float2 sharpen(float2 uv, float2 size)
{
    float2 s = uv * size - 0.5;
    float2 whole = floor(s);
    float2 part = s - whole;
    float2 width = max(fwidth(s), float2(1e-6, 1e-6));
    float2 ramp = saturate((part - 0.5) / width + 0.5);
    return (whole + ramp + 0.5) / size;
}

/// The signed distance from `p` to a rectangle of half extent `halfExtent` centred on
/// the origin, with the corner in `p`'s quadrant rounded by its own radius.
///
/// Negative inside. Y grows downwards, as everywhere in the UI, so `p.y > 0` is
/// the bottom half and the radii are indexed top-left, top-right, bottom-right,
/// bottom-left. The rounded box is Inigo Quilez's, with the radius chosen per
/// quadrant.
float roundedBoxDistance(float2 p, float2 halfExtent, float4 radii)
{
    float top = p.x > 0.0 ? radii.y : radii.x;
    float bottom = p.x > 0.0 ? radii.z : radii.w;
    float radius = p.y > 0.0 ? bottom : top;
    float2 q = abs(p) - halfExtent + radius;
    return min(max(q.x, q.y), 0.0) + length(max(q, float2(0.0, 0.0))) - radius;
}

[shader("fragment")]
float4 fragmentMain(UiOutput input) : SV_Target
{
    // Both atlases sampled and the derivative taken first, unconditionally —
    // see the header. A solid quad's UV is (0, 0) and a rounded rect's is a
    // pixel offset; what either samples is fetched and then not used.
    float glyph = glyphAtlas.Sample(glyphSampler, input.uv);
    uint imageWidth;
    uint imageHeight;
    imageAtlas.GetDimensions(imageWidth, imageHeight);
    float2 imageSize = float2(float(imageWidth), float(imageHeight));
    float4 texel = imageAtlas.Sample(imageSampler, sharpen(input.uv, imageSize));

    float primitive = input.shape.w;
    float4 color = input.color;

    // Solid colour is `color` as it is. A glyph multiplies coverage into alpha.
    color.a *= primitive == PRIMITIVE_GLYPH ? glyph : 1.0;

    // An image multiplies the whole texel by the tint.
    color = primitive == PRIMITIVE_IMAGE ? texel * input.color : color;

    // A rounded rect: coverage from the distance to its edge, and the border
    // from the distance to an edge `border width` further in, each ramped over
    // one pixel so the corners are smooth with multisampling off. A fragment
    // centre half a pixel inside a straight edge is fully covered, so the
    // rectangle's straight sides are as crisp as a `Rect`'s.
    float distance = roundedBoxDistance(input.uv, input.shape.xy, input.radii);
    float coverage = saturate(0.5 - distance);
    float inner = saturate(0.5 - (distance + input.shape.z));
    float4 rounded = lerp(input.border, input.color, input.shape.z > 0.0 ? inner : 1.0);
    rounded.a *= coverage;
    color = primitive == PRIMITIVE_ROUNDED_RECT ? rounded : color;

    // The clip, last. Fragment centres sit half a pixel inside a whole-pixel
    // clip edge, so the comparison has half a pixel of margin either way.
    if (input.screen.x < input.clip.x || input.screen.y < input.clip.y
        || input.screen.x >= input.clip.z || input.screen.y >= input.clip.w)
    {
        discard;
    }

    return color;
}
