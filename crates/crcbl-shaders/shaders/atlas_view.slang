// crcbl-targets: spirv, wgsl, msl, dxil

// `docs/plan/sample/18-sundial.md`'s atlas viewer: the shadow atlas itself drawn
// over the frame, so which slot holds which map is a thing a reviewer can look
// at rather than infer.
//
// # What no other debug view can show
//
// `crcbl_render::DebugView`'s other views are all readouts of the *frame*: a
// cluster's error, a surface's normal, the cascade a fragment's shadow came
// from. Every one of them is silent about the thing on the writing side — a
// tile that was never rendered into, one rendered at the wrong viewport, one a
// light was given and then not drawn into. A frame lit through such a tile is
// fully lit and entirely plausible, which is the failure
// `crates/crcbl/tests/forward_e2e/shadow.rs` opens by naming. This pass draws
// the depth in the atlas, so an empty tile is a dark rectangle where a drawn one
// is not.
//
// # It is a pass rather than a branch in `mesh.slang`
//
// Every other replacing view is a branch in the colour pass, because every other
// view is a function of the fragment being shaded. This one is not a function of
// any fragment: the atlas is one image the whole frame shares, and a pixel of
// this picture is an atlas texel rather than a piece of geometry. A branch in
// the colour pass could only have drawn it where geometry happened to cover, so
// what a reviewer saw of the atlas would depend on where they stood.
//
// # `Load`, and no sampler at all
//
// A stored depth, fetched at an integer coordinate — `contact_shadows.slang`'s
// binding block makes the same argument, and `mesh.slang`'s
// `sun_penumbra_texels` is the precedent for reading this particular image that
// way. Filtering four depths would average across a silhouette and draw a height
// nothing in the scene occupies, which for a *diagnostic* is the one thing that
// must not happen: the picture would stop being evidence about the atlas.
//
// The atlas's only sampler is the comparison one the shadow filter uses, and a
// comparison sampler cannot return a depth — so reading by `Load` is also what
// lets this pass bind the image the forward pass already binds without creating
// a second sampler object for it.
//
// # No `#include`
//
// `bloom_composite.slang` states the rule: the manifest hashes one source per
// artifact, so an include would be a file whose edits nothing downstream
// notices. The fullscreen triangle below is `fxaa.slang`'s, copied.

/// Slots the shadow atlas has, matching
/// `crcbl_shaders::mesh::SHADOW_ATLAS_TILES` and `mesh.slang`'s own copy.
///
/// This file sizes `AtlasViewParams::rect` with it, so it is a layout number
/// here exactly as it is there: a block sized differently on the two sides puts
/// nothing after it at the right offset. `crcbl_shaders::atlas_view`'s
/// `the_slot_count_matches_the_one_the_host_declares` is what holds the copy.
static const uint SHADOW_ATLAS_TILES = 16;

/// Everything the pass takes from the CPU.
///
/// `crcbl_shaders::atlas_view::AtlasViewParams` is the Rust mirror and
/// `crcbl_render::atlas_view` is what writes the buffer, once per frame.
struct AtlasViewParams
{
    /// Where the atlas is drawn in the frame, in pixels: `xy` the top-left
    /// corner and `zw` the size. The renderer letterboxes it, so `zw` has the
    /// atlas's own aspect whatever shape the frame is.
    float4 view;
    /// The atlas's extent in texels, which is what turns a position inside
    /// `view` into the integer coordinate `Load` wants. `zw` is unread padding.
    float4 atlas;
    /// Where each atlas slot's map is, straight off
    /// `FrameUniforms::shadow_atlas_rect`: `xy` the map's size as a fraction of
    /// the atlas and `zw` its origin, which is the shape that member documents.
    ///
    /// A slot no map was rendered into carries a **zero** rectangle, and that is
    /// what the border loop below tests: a zero-sized rectangle gets no border,
    /// so the borders in the picture are exactly the occupied slots.
    ///
    /// **Last in the block**, on `FrameUniforms::shadow_atlas_rect`'s terms.
    float4 rect[SHADOW_ATLAS_TILES];
};

// Declaration order is binding order and it is load-bearing on Metal — see
// `crcbl_shaders::declaration_order`, which lints it over every source.

[[vk::binding(0, 0)]]
ConstantBuffer<AtlasViewParams> params;

/// The sun's cascades and the lit tiles beside them, as depth.
///
/// `DepthTexture2D` for the seam's `SampleType::Depth` — the only sample type
/// WebGPU will bind a `D32Float` view through, and the type `mesh.slang`'s own
/// binding of this image carries the argument for.
[[vk::binding(1, 0)]]
DepthTexture2D shadow_atlas;

struct FullscreenOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

/// The reversed-Z far plane: the value the shadow pass clears the atlas to.
///
/// A texel still holding it is a texel nothing was drawn into, which is the
/// distinction this whole picture exists to make.
/// `crcbl_shaders::atlas_view::DEPTH_CLEAR` is the Rust mirror.
static const float DEPTH_CLEAR = 0.0;

/// What the frame outside the atlas's rectangle draws — black, so the
/// letterbox reads as absence rather than as a very empty tile.
static const float SURROUND = 0.0;

/// What a texel still at [`DEPTH_CLEAR`] draws.
///
/// Not zero, so an empty *tile* is still distinguishable from the letterbox
/// around the atlas, and far below [`OCCUPIED_FLOOR`], so no depth a caster
/// could have written lands anywhere near it.
static const float EMPTY_GREY = 0.06;

/// The darkest a texel something **was** drawn into may draw.
///
/// The mapping is `lerp(OCCUPIED_FLOOR, 1.0, depth)` rather than `depth`
/// itself, and this floor is the whole reason: a caster at the far end of a
/// cascade stores a depth just above the clear, and drawn as its own value it
/// would be the same black as the tile nobody rendered into. Lifting the range
/// costs the picture nothing a reviewer reads — what is being looked at is
/// *which* texels hold a caster and how the heights are laid out, not their
/// absolute distance — and buys the one comparison the picture is for.
static const float OCCUPIED_FLOOR = 0.3;

/// How wide the border around an occupied tile is, in **frame pixels**.
///
/// Frame pixels rather than atlas texels, and that is the difference between a
/// border and nothing: the atlas is thousands of texels across and is drawn into
/// a few hundred, so a border a texel wide would fall between the samples this
/// pass takes and be invisible at every frame size the engine draws.
static const float BORDER_PIXELS = 2.0;

/// The colour an occupied tile's border draws.
///
/// **A colour rather than a grey**, deliberately: every other texel of this
/// picture is a grey, so a border on the same axis would be one more depth to
/// read past. Amber is off that axis entirely — red leads blue by more than any
/// grey can — which is what makes the tile grid readable over an atlas whose
/// contents are dark, bright, or both.
static const float3 BORDER_TINT = float3(1.0, 0.55, 0.15);

[shader("vertex")]
FullscreenOutput vertexMain(uint index: SV_VertexID)
{
    FullscreenOutput output;
    // index 0 → uv (0,0), index 1 → (2,0), index 2 → (0,2). `tonemap.slang`
    // carries the *why* of this shape and of the -2.0 on Y.
    output.uv = float2(float((index << 1) & 2), float(index & 2));
    output.position = float4(output.uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}

[shader("fragment")]
float4 fragmentMain(FullscreenOutput input) : SV_Target
{
    // `SV_Position.xy`, which is a pixel *centre* — the same value
    // `mesh.slang`'s `shadow_filter_mode` places the comparison seam with, and
    // the space `params.view` is written in.
    float2 pixel = input.position.xy;
    float2 inside = pixel - params.view.xy;
    // Alpha 1.0 on every path out of this pass: what it writes is composited
    // opaque, exactly as `tonemap.slang` says.
    if (any(inside < float2(0.0, 0.0)) || any(inside >= params.view.zw))
    {
        return float4(SURROUND, SURROUND, SURROUND, 1.0);
    }

    // The tile grid first, so a border is drawn over the texel it covers rather
    // than under it: a one-texel line that lost to the depth beneath it would
    // disappear exactly where the atlas is bright, which is where the tiles are
    // occupied and the grid is worth having.
    for (uint slot = 0u; slot < SHADOW_ATLAS_TILES; ++slot)
    {
        float4 rect = params.rect[slot];
        // A slot no map was rendered into. Skipped rather than drawn empty,
        // because a zero rectangle has no edges to draw and its texels are the
        // clear value the grey below already says.
        if (rect.x <= 0.0 || rect.y <= 0.0)
        {
            continue;
        }
        float2 tile_min = rect.zw * params.view.zw;
        float2 tile_max = tile_min + rect.xy * params.view.zw;
        if (any(inside < tile_min) || any(inside >= tile_max))
        {
            continue;
        }
        float2 edge = min(inside - tile_min, tile_max - inside);
        if (min(edge.x, edge.y) < BORDER_PIXELS)
        {
            return float4(BORDER_TINT, 1.0);
        }
    }

    float2 extent = params.atlas.xy;
    // `min` against the last texel rather than trusting the bound above: that
    // one is in pixels and this is a float product, and an out-of-range `Load`
    // is undefined rather than clamped — `mesh.slang`'s `sun_penumbra_texels`
    // clamps its own coordinate for the same reason.
    int2 coord = int2(min(inside / params.view.zw * extent, extent - float2(1.0, 1.0)));
    float depth = shadow_atlas.Load(int3(coord, 0));
    float grey = depth > DEPTH_CLEAR ? lerp(OCCUPIED_FLOOR, 1.0, depth) : EMPTY_GREY;
    return float4(grey, grey, grey, 1.0);
}
