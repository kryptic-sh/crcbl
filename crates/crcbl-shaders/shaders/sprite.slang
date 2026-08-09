// crcbl-targets: spirv, wgsl, msl, dxil

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
// would mean the same in the other direction.
//
// # Why the batch's base instance is a *constant* and not `firstInstance`
//
// A batch is a run of consecutive sprites sharing a sheet, and each one is one
// draw. The obvious way to point a draw at its slice of the frame's instance
// buffer is the draw's own `firstInstance`, and that is what this pass did until
// it was found to render every batch after the first from the *first* batch's
// instances. The cause is that `SV_InstanceID` does not mean the same thing on
// the two targets this file is compiled for:
//
// * On **SPIR-V**, `slangc` lowers it to `InstanceIndex - BaseInstance`, which is
//   HLSL's definition — the id counts from the start of the *draw* and excludes
//   `StartInstanceLocation`. Indexing a frame-wide buffer with it restarts at 0
//   for every batch.
// * On **WGSL**, `slangc` lowers it straight to `@builtin(instance_index)`, and
//   WebGPU defines that builtin to *start at* `firstInstance`. So the same source
//   indexes absolutely there.
//
// Both cannot be right, and nothing in this file can be written that is correct
// on both while `firstInstance` is non-zero. So the pass stopped using it:
// every draw is `draw(0..6, 0..count)`, which makes the two lowerings agree —
// with a zero base they are the same number — and the batch's offset into the
// buffer arrives in `SpriteConstants::base`, through a dynamic-offset binding of
// set 0. Set 0 and set 1 are then both rebound per batch.
//
// The alternative was to add `SV_StartInstanceLocation` back on for SPIR-V only.
// It compiles, but `slangc` rejects that semantic outright for WGSL, so the
// source would have to be `#if`-split per target — and, worse, the WGSL half
// would then be relying on Slang's WGSL lowering *disagreeing* with its SPIR-V
// one. A Slang release that made the two agree would silently break the browser.

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
    /// **The index of this batch's first sprite in the frame's instance buffer.**
    ///
    /// Added to `SV_InstanceID` — which is zero-based within the draw on both
    /// targets, because every draw this pass emits has `firstInstance == 0` — to
    /// give the absolute index. The header records why the draw's own
    /// `firstInstance` cannot carry it.
    ///
    /// This is the only field that differs between the blocks of one frame, and
    /// it is why there is a block per *batch* rather than per frame: the whole
    /// struct is re-emitted at each dynamic-offset stride so that one binding can
    /// address any of them.
    uint base;
    /// Pads the block to a multiple of 16 bytes, which `std140` requires of a
    /// uniform buffer's size and which naga enforces where a driver would not.
    uint pad;
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
    /// **How this sprite's sheet is sampled, how big it is, and how far the
    /// sprite is turned.**
    ///
    /// `xy` is the sheet image's size in texels, `z` is the sample mode —
    /// `1.0` for `crcbl_sprite::SampleMode::Pixel` and `0.0` for
    /// `SampleMode::Smooth` — and `w` is the rotation in **radians,
    /// counter-clockwise, about the quad's own centre**.
    ///
    /// The first two are properties of the *sheet*, not of the sprite, and they
    /// still travel on the instance — because the alternative is worse in every
    /// direction. The mode cannot come from the sampler: set 1 binds one
    /// `SamplerState` and a shader has no way to ask which filter it got. It
    /// could come from a second uniform block rebound per batch, which is a
    /// third descriptor set and a per-batch bind for eight bytes; or from a
    /// second pipeline, which is the `ui.slang`/`ui_tier_b.slang` permutation
    /// mistake again, this time with the two halves differing by one `lerp`.
    /// Two `float4` lanes of a buffer that is already written per sprite per
    /// frame cost one 16-byte store and buy one pipeline, one sampler and one
    /// descriptor set for both modes.
    ///
    /// **`w` was padding and is now the angle**, which cost nothing at all: the
    /// instance is still four `float4`s and 64 bytes, so no buffer, no stride
    /// and no layout rule moved. Only the vertex stage reads it —
    /// `SpriteVarying::sheet` still carries `xyz` — so the fragment stage is
    /// untouched by rotation.
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
    // `instance` counts from zero within this draw on both targets — see the
    // header — and `constants.base` is where this batch starts in the frame's
    // buffer. Neither half is optional: dropping the base draws every batch from
    // the first one's instances, which is the bug this arrangement exists for.
    SpriteInstance s = sprites[instance + constants.base];
    float2 corner = CORNERS[vertex];

    // The quad in world units: minimum corner plus the corner's fraction of the
    // size. z is 0 — sprites composite in submission order with no depth test,
    // and layer ordering is the caller's, not this shader's.
    float2 world = s.rect.xy + corner * s.rect.zw;

    // # Rotation, about the quad's own centre
    //
    // The pivot is the centre and nothing else. A ship, a rock and a bullet all
    // turn about themselves, an arbitrary pivot is another two floats on a lane
    // that has exactly one left, and a caller that genuinely wants to swing a
    // sprite about a distant point can place `rect` accordingly — the rotation
    // of a rectangle about an outside point is the same rectangle rotated about
    // its centre and then translated.
    //
    // **The zero case is the expression above, untouched, and that is
    // deliberate.** The tempting spelling is one formula for both —
    // `centre + rotate(corner - 0.5) * size` — which is the same number in real
    // arithmetic and *not* the same float: subtracting half the size and adding
    // it back lands an ulp away from `rect.xy + corner * rect.zw`, and an ulp of
    // world position is a quad edge that can fall on the other side of a
    // fragment centre. Every golden image in the suite is a sprite with no
    // angle, so "unchanged where the angle is zero" has to be a property of the
    // code rather than a hope about rounding.
    float angle = s.sheet.w;
    if (angle != 0.0)
    {
        float2 pivot = s.rect.zw * 0.5;
        float2 offset = corner * s.rect.zw - pivot;
        float sinA = sin(angle);
        float cosA = cos(angle);
        // Counter-clockwise with Y up, which is what a positive angle means
        // everywhere above this file.
        float2 turned = float2(
            offset.x * cosA - offset.y * sinA,
            offset.x * sinA + offset.y * cosA
        );
        world = s.rect.xy + pivot + turned;
    }

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
    //
    // # And what that means once the quad is turned
    //
    // Rounding each corner is only defined because an unrotated quad's corners
    // are the corners of an axis-aligned rectangle: rounding them keeps it a
    // rectangle, and `ceil(a - 0.5) == round(a)` keeps its coverage. A rotated
    // quad has no such rectangle. Rounding its four corners independently moves
    // each of them up to half a pixel in a *different* direction, which shears
    // the quad, changes its size and changes its effective angle — and does so
    // differently at every position, so a ship turning slowly would visibly
    // wobble and breathe. That is worse than the crawl the snap exists to
    // remove.
    //
    // So a rotated quad is snapped as a **rigid body**: the sprite's centre is
    // projected, the offset that would put *it* on the pixel grid is computed
    // once, and every corner is translated by that same offset. Shape, size and
    // angle survive exactly; what goes away is the sub-pixel drift of the whole
    // sprite against the fragment grid, which is the part of the crawl that
    // translation causes and the part a moving rock or ship actually shows. The
    // rest — a hard edge stepping as the angle changes — is inherent to
    // rasterising a turning rectangle without coverage, and no snap addresses
    // it.
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
        if (angle != 0.0)
        {
            // The rigid snap. One extra transform of one point per vertex, and
            // only for a sprite that is actually turned.
            float2 centre = s.rect.xy + s.rect.zw * 0.5;
            float4 centreClip = mul(constants.view_proj, float4(centre, 0.0, 1.0));
            float2 turnedSnap = pixels;
            if (centreClip.w > 0.0)
            {
                float2 centrePixels =
                    ((centreClip.xy / centreClip.w) * 0.5 + 0.5) * viewport;
                turnedSnap = pixels + (round(centrePixels) - centrePixels);
            }
            snapped = lerp(pixels, turnedSnap, pixelMode);
        }
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
///
/// # Rotation needs nothing here, and this is why
///
/// `fwidth` is a **per-fragment** derivative of the interpolated value, so it is
/// taken from whatever the vertex stage actually produced: turn the quad and the
/// UV gradient turns with it, and `s` still advances by the same number of texels
/// per fragment. Nothing in this function knows or needs to know the angle.
///
/// The one thing rotation does change is `fwidth`'s *shape*. It is
/// `abs(ddx) + abs(ddy)` — an L1 norm — so for an axis-aligned quad one term is
/// zero and it reports the scale exactly, while at 45° both terms are the scale
/// over root two and it reports root two times the scale. The band is therefore
/// up to ~1.41 fragments wide on the diagonal instead of 1. That is the safe
/// direction (a band narrower than a fragment aliases; a slightly wider one only
/// softens), it never reaches zero for a quad with area, and the `max` below
/// still catches the degenerate case.
///
/// Measured on the `sprite_rotation` golden, whose six sprites are 32 device
/// pixels per texel across and 16 down: at 0°, 90°, 180° and 270° **zero** of
/// the ~1860 interior pixels are anything but one of the sheet's four texel
/// colours — the quarter turns are as exactly flat as no turn at all — and at
/// 45° it is 66 of 1870, a crossover two pixels wide measured along a scanline
/// that crosses the boundary diagonally, which is the ~1.41 fragments
/// perpendicular that the L1 norm predicts.
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
