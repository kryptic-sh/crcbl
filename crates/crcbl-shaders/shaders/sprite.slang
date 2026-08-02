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
    /// The target's size in device pixels, and the whole reason the vertex
    /// stage can snap: clip space says nothing about how big a pixel is, so
    /// rounding a position to the pixel grid needs the map from NDC to that
    /// grid, and this is it.
    float2 viewport;
    /// Pads the block to a multiple of 16 bytes, which `std140` requires of a
    /// uniform buffer's size and which naga enforces where a driver would not.
    float2 pad;
};

[[vk::binding(0, 0)]]
ConstantBuffer<SpriteConstants> constants;

/// One sprite, matching `crcbl_render::sprite_pass::SpriteInstance`.
///
/// Four `float4`s — 64 bytes, no padding on any layout rule — for the same
/// reason `MeshVertex` is three `float4`s: a `float3` or a `float2` pair in a
/// structured buffer acquires alignment padding the CPU side then has to
/// reproduce exactly, and a layout that cannot drift is worth the wasted lane.
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
    /// **How this sprite's sheet is sampled, and how big it is.**
    ///
    /// `xy` is the sheet image's size in texels and `z` is the sample mode:
    /// `1.0` for `crcbl_sprite::SampleMode::Pixel` and `0.0` for
    /// `SampleMode::Smooth`. `w` pads the lane.
    ///
    /// Both are properties of the *sheet*, not of the sprite, and they still
    /// travel on the instance — because the alternative is worse in every
    /// direction. The mode cannot come from the sampler: set 1 binds one
    /// `SamplerState` and a shader has no way to ask which filter it got. It
    /// could come from a second uniform block rebound per batch, which is a
    /// third descriptor set and a per-batch bind for eight bytes; or from a
    /// second pipeline, which is the `ui.slang`/`ui_tier_b.slang` permutation
    /// mistake again, this time with the two halves differing by one `lerp`.
    /// Two `float4` lanes of a buffer that is already written per sprite per
    /// frame cost one 16-byte store and buy one pipeline, one sampler and one
    /// descriptor set for both modes.
    float4 sheet;
};

[[vk::binding(1, 0)]]
StructuredBuffer<SpriteInstance> sprites;

/// The sheet: `Rgba8UnormSrgb` or `Rgba8Unorm`, straight (not premultiplied)
/// alpha.
[[vk::binding(0, 1)]]
Texture2D<float4> sheet;

/// **Always linear, for both modes.** `SampleMode::Pixel` is sharp-bilinear,
/// which is linear sampling of a bent UV — see `fragmentMain` — so a nearest
/// sampler would defeat it exactly. There is one sampler in the whole pass and
/// nothing in this file needs to know which it got.
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
    /// `xy` is the sheet's texel size and `z` the sample mode, straight off
    /// `SpriteInstance::sheet`.
    ///
    /// `nointerpolation` because both are per-*instance* constants: every one of
    /// the quad's six vertices writes the same value, so interpolating them is
    /// arithmetic whose answer is already known, and a mode that arrived as
    /// `0.99999997` would make an exact `lerp` weight into an almost-exact one.
    nointerpolation float3 sheet : TEXCOORD1;
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

    float4 clip = mul(constants.view_proj, float4(world, 0.0, 1.0));

    // # Snapping the quad to whole device pixels
    //
    // A sprite whose edge falls at x = 40.5 covers half of fragment 40, and the
    // rasteriser has no coverage to express that: the fragment is either in or
    // out. Move the sprite by a thousandth of a unit and the edge crosses 40.5,
    // the fragment flips, and the sprite's outline visibly jitters by a whole
    // pixel while the art inside it slides smoothly. Rounding each corner to the
    // pixel grid removes the flip — the edge is always *on* a boundary, so it
    // never straddles one — at the cost of the sprite's position being quantised
    // to whole pixels, which is what pixel art wants and is exactly why this is
    // gated on the mode.
    //
    // `pixelMode` is 1 for `SampleMode::Pixel` and 0 for `Smooth`, so the `lerp`
    // leaves a smooth sprite's clip position bit-for-bit as it was.
    float pixelMode = s.sheet.z;
    // A zero viewport would divide by zero. `begin_frame` always writes the
    // target's real extent, so this is a floor and not a fallback.
    float2 viewport = max(constants.viewport, float2(1.0, 1.0));
    if (clip.w > 0.0)
    {
        // NDC → the device-pixel grid → back. The `w` multiply at the end undoes
        // the divide, so a perspective camera — which a sprite is not normally
        // under, but nothing here forbids — snaps in screen space rather than in
        // a space where a pixel is not a fixed size.
        float2 ndc = clip.xy / clip.w;
        float2 pixels = (ndc * 0.5 + 0.5) * viewport;
        float2 snapped = lerp(pixels, round(pixels), pixelMode);
        clip.xy = ((snapped / viewport) * 2.0 - 1.0) * clip.w;
    }

    SpriteVarying output;
    output.position = clip;
    output.uv = uv;
    output.tint = s.tint;
    output.sheet = s.sheet.xyz;
    return output;
}

/// Bends `uv` so a linear sampler behaves like nearest everywhere except a band
/// one fragment wide at each texel boundary — *sharp bilinear*.
///
/// # The derivation
///
/// A linear sampler at texel-space position `p` blends the two texels either
/// side of `p - 0.5` with weight `frac(p - 0.5)`: the weight is 0 at a texel
/// *centre* and sweeps through the whole 0..1 range across the gap between two
/// centres. That sweep is the problem — a 2×2 sheet blown up to 64 pixels is a
/// gradient across the middle 32 of them, which is not what the art says.
///
/// So write `s = p - 0.5`, whose integers are the texel centres and whose
/// half-integers are the texel *boundaries*, and replace `frac(s)` with a
/// steepened copy of itself: flat 0 until just before the boundary at 0.5, a
/// straight ramp across it, flat 1 after. Feed the result back as a UV and the
/// hardware does the blending, so there is still exactly one `Sample`.
///
/// **The ramp's width is `fwidth(s)`, which is the number of texels one fragment
/// spans** — the derivative of the texel-space position with respect to the
/// screen. Dividing the offset from the boundary by it converts texels into
/// fragments, so the ramp is one fragment wide *in screen space* whatever the
/// scale: 32 screen pixels per texel gives a band of 1/32 of a texel, and 2.5
/// screen pixels per texel gives 0.4 of one. That fixed screen-space width is
/// the entire point. Nearest gives no ramp at all, so a boundary lands wherever
/// the fragment grid happens to put it and the art's pixels come out four screen
/// pixels wide next to five; plain linear gives a ramp a whole texel wide, so
/// there are no art pixels left. One fragment is the only width that is neither.
///
/// At a whole scale the arithmetic collapses to nearest exactly, and not
/// approximately: with `n` screen pixels per texel the fragment centres nearest a
/// boundary sit `0.5/n` texels either side of it, which is exactly the half-width
/// of the ramp, so they evaluate to a clean 0 and 1.
float2 sharpen(float2 uv, float2 size)
{
    // Texel-space position, shifted so integers are texel centres.
    float2 s = uv * size - 0.5;
    float2 whole = floor(s);
    float2 part = s - whole;
    // Texels per fragment. Floored away from zero because a degenerate
    // derivative — a quad collapsed to nothing, or a driver's helper lanes on a
    // one-fragment triangle — would otherwise divide by it.
    float2 width = max(fwidth(s), float2(1e-6, 1e-6));
    // The ramp: `part == 0.5` is the boundary, and `width` texels of run either
    // side of it is one fragment of run in total.
    float2 ramp = saturate((part - 0.5) / width + 0.5);
    return (whole + ramp + 0.5) / size;
}

[shader("fragment")]
float4 fragmentMain(SpriteVarying input) : SV_Target
{
    // A zero-sized sheet cannot exist — `register_sheet` uploads a real image —
    // but the divide in `sharpen` is worth a floor anyway.
    float2 size = max(input.sheet.xy, float2(1.0, 1.0));

    // Both UVs are computed for every fragment and one of them is chosen by
    // arithmetic, because there is deliberately no branch anywhere in this
    // stage. WGSL requires `textureSample` to be reached from uniform control
    // flow; Dawn enforces that where naga does not, and one module it rejects
    // invalidates the pipeline, the command buffer and therefore the whole
    // frame. That is what left a browser drawing a black canvas while the game
    // ran normally — see `ui.slang`'s fragment stage, which is the same bug
    // written down after the fact.
    //
    // `fwidth` inside `sharpen` has the same requirement for the same reason: a
    // derivative is taken across a 2×2 quad of fragments, so a lane that skipped
    // it would leave its neighbours reading an undefined value.
    //
    // `input.sheet.z` is exactly 1.0 or exactly 0.0 — it is `nointerpolation`,
    // so no interpolator has been near it — which makes this `lerp` a selection
    // and leaves `Smooth` sampling precisely the UV the vertex stage produced.
    float2 uv = lerp(input.uv, sharpen(input.uv, size), input.sheet.z);
    float4 texel = sheet.Sample(sheetSampler, uv);

    // Straight alpha in, straight alpha out: the colour is *not* premultiplied
    // here, because the pipeline blends with `src * a + dst * (1 - a)` and doing
    // the multiply in both places darkens every edge pixel.
    return texel * input.tint;
}
